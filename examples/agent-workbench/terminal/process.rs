use portable_pty::{Child, ChildKiller, CommandBuilder, ExitStatus, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    num::NonZeroU16,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
use tokio::sync::watch;

use super::screen::{Frame, Keyboard};

const FRAME: Duration = Duration::from_millis(16);
const SCROLLBACK: usize = 1500;
type Killer = Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>;

pub(super) enum Input {
    Text(Vec<u8>),
    Resize(PtySize),
    Scroll(i32),
}

enum Message {
    Input(Input),
    Output(Vec<u8>),
    Eof,
}

pub(super) struct Session {
    commands: SyncSender<Message>,
    cancelled: Arc<AtomicBool>,
    killer: Arc<Killer>,
    pub(super) output: watch::Receiver<Frame>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Ok(mut killer) = self.killer.lock()
            && let Some(mut child) = killer.take()
        {
            let _ = child.kill();
        }
    }
}

impl Session {
    pub(super) fn start(command: CommandBuilder, size: PtySize) -> Result<Self, String> {
        let (commands, inbox) = mpsc::sync_channel(64);
        let (send, output) = watch::channel(Frame::status("正在启动终端…"));
        let cancelled = Arc::new(AtomicBool::new(false));
        let killer = Arc::new(Killer::default());
        let worker_killer = killer.clone();
        let reader = commands.clone();
        let stop = cancelled.clone();
        thread::Builder::new()
            .name("xrack-terminal".into())
            .spawn(move || {
                if let Err(error) = run(command, size, reader, inbox, &stop, &worker_killer, &send)
                {
                    send.send_modify(|frame| frame.finish(format!("终端错误（{error}）")));
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            commands,
            cancelled,
            killer,
            output,
        })
    }

    pub(super) fn send(&self, input: Input) -> Result<(), String> {
        self.commands
            .try_send(Message::Input(input))
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => "终端暂忙，请重试".into(),
                mpsc::TrySendError::Disconnected(_) => "终端已退出".into(),
            })
    }
}

struct RunningChild<'a>(Box<dyn Child + Send + Sync>, &'a Killer);

impl RunningChild<'_> {
    fn status(&mut self) -> std::io::Result<Option<ExitStatus>> {
        let mut killer = self
            .1
            .lock()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let status = self.0.try_wait()?;
        if status.is_some() {
            killer.take();
        }
        Ok(status)
    }
}

impl Drop for RunningChild<'_> {
    fn drop(&mut self) {
        if let Ok(mut killer) = self.1.lock() {
            killer.take();
        }
        if self.0.try_wait().is_ok_and(|status| status.is_some()) {
            return;
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run(
    command: CommandBuilder,
    mut size: PtySize,
    reader: SyncSender<Message>,
    inbox: Receiver<Message>,
    cancelled: &AtomicBool,
    killer: &Killer,
    output: &watch::Sender<Frame>,
) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let pair = native_pty_system()
        .openpty(size)
        .map_err(|error| error.to_string())?;
    let mut read_end = pair
        .master
        .try_clone_reader()
        .map_err(|error| error.to_string())?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|error| error.to_string())?;
    let mut child = RunningChild(
        pair.slave
            .spawn_command(command)
            .map_err(|error| error.to_string())?,
        killer,
    );
    {
        let mut killer = killer.lock().map_err(|error| error.to_string())?;
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        *killer = Some(child.0.clone_killer());
    }
    drop(pair.slave);
    thread::Builder::new()
        .name("xrack-terminal-reader".into())
        .spawn(move || {
            let mut chunk = [0; 4096];
            loop {
                match read_end.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(count) => {
                        if reader
                            .send(Message::Output(chunk[..count].to_vec()))
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            let _ = reader.send(Message::Eof);
        })
        .map_err(|error| error.to_string())?;
    let mut parser = vt100::Parser::new(cells(size.rows), cells(size.cols), SCROLLBACK);
    output.send_replace(Frame::read(parser.screen()));
    let mut last_frame = Instant::now();
    let mut dirty = false;
    let mut eof = false;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        let message = inbox.recv_timeout(FRAME.saturating_sub(last_frame.elapsed()));
        match message {
            Ok(Message::Output(bytes)) => {
                parser.process(&bytes);
                dirty = true;
            }
            Ok(Message::Eof) => eof = true,
            Ok(Message::Input(Input::Text(bytes))) => {
                parser.screen_mut().set_scrollback(0);
                writer
                    .write_all(&bytes)
                    .map_err(|error| error.to_string())?;
                dirty = true;
            }
            Ok(Message::Input(Input::Resize(next))) if next != size => {
                pair.master
                    .resize(next)
                    .map_err(|error| error.to_string())?;
                parser.set_size(cells(next.rows), cells(next.cols));
                size = next;
                dirty = true;
            }
            Ok(Message::Input(Input::Scroll(lines))) => {
                let screen = parser.screen_mut();
                let offset = if lines == 0 {
                    0
                } else {
                    screen.scrollback().saturating_add_signed(lines as isize)
                };
                screen.set_scrollback(offset);
                dirty = true;
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
        if eof && let Some(status) = child.status().map_err(|error| error.to_string())? {
            let mut frame = Frame::read(parser.screen());
            frame.finish(format!("终端已退出（{status}）"));
            output.send_replace(frame);
            return Ok(());
        }
        if last_frame.elapsed() >= FRAME {
            if dirty {
                output.send_replace(Frame::read(parser.screen()));
                dirty = false;
            }
            last_frame = Instant::now();
        }
    }
}

fn cells(value: u16) -> NonZeroU16 {
    NonZeroU16::new(value).unwrap_or(NonZeroU16::MIN)
}

pub(super) fn key_input(event: &xrack_runtime::KeyEvent, mode: Keyboard) -> Option<Input> {
    let key = event.key.as_str();
    let modifiers = event.modifiers;
    if modifiers.meta
        || modifiers.primary && key.eq_ignore_ascii_case("v")
        || modifiers.primary && key.eq_ignore_ascii_case("j")
    {
        return None;
    }
    if modifiers.shift && matches!(key, "PageUp" | "PageDown") {
        let rows = i32::from(mode.rows);
        return Some(Input::Scroll(if key == "PageUp" { rows } else { -rows }));
    }
    if modifiers.control && !modifiers.alt {
        let mut chars = key.chars();
        if let (Some(character), None) = (chars.next(), chars.next()) {
            let control = character.to_ascii_uppercase();
            if ('@'..='_').contains(&control) || control == ' ' {
                return Some(Input::Text(vec![(control as u8) & 0x1f]));
            }
        }
    }
    let prefix = if mode.application_cursor {
        "\x1bO"
    } else {
        "\x1b["
    };
    let bytes = match key {
        "Enter" => "\r",
        "Backspace" => "\x7f",
        "Delete" => "\x1b[3~",
        "Escape" => "\x1b",
        "Tab" if modifiers.shift => "\x1b[Z",
        "Tab" => "\t",
        "Home" => "\x1b[H",
        "End" => "\x1b[F",
        "Insert" => "\x1b[2~",
        "PageUp" => "\x1b[5~",
        "PageDown" => "\x1b[6~",
        "F1" => "\x1bOP",
        "F2" => "\x1bOQ",
        "F3" => "\x1bOR",
        "F4" => "\x1bOS",
        "F5" => "\x1b[15~",
        "F6" => "\x1b[17~",
        "F7" => "\x1b[18~",
        "F8" => "\x1b[19~",
        "F9" => "\x1b[20~",
        "F10" => "\x1b[21~",
        "F11" => "\x1b[23~",
        "F12" => "\x1b[24~",
        "ArrowUp" | "ArrowDown" | "ArrowRight" | "ArrowLeft" => {
            let final_byte = match key {
                "ArrowUp" => 'A',
                "ArrowDown" => 'B',
                "ArrowRight" => 'C',
                _ => 'D',
            };
            return Some(Input::Text(format!("{prefix}{final_byte}").into_bytes()));
        }
        _ => return None,
    };
    Some(Input::Text(bytes.as_bytes().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use xrack_runtime::{KeyEvent, KeyModifiers};

    #[test]
    fn terminal_keys_map_control_and_application_cursor_sequences() {
        let key = |name: &str, modifiers| KeyEvent {
            key: name.into(),
            modifiers,
            ..KeyEvent::default()
        };
        let normal = Keyboard {
            rows: 10,
            ..Keyboard::default()
        };
        let application = Keyboard {
            application_cursor: true,
            ..normal
        };
        assert!(
            matches!(key_input(&key("ArrowLeft", KeyModifiers::default()), application),
            Some(Input::Text(bytes)) if bytes == b"\x1bOD")
        );
        let control = KeyModifiers {
            control: true,
            ..KeyModifiers::default()
        };
        assert!(matches!(key_input(&key("c", control), normal),
            Some(Input::Text(bytes)) if bytes == b"\x03"));
        let shift = KeyModifiers {
            shift: true,
            ..KeyModifiers::default()
        };
        assert!(matches!(
            key_input(&key("PageUp", shift), normal),
            Some(Input::Scroll(10))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn native_pty_runs_a_shell_with_working_directory_resize_and_interrupt()
    -> Result<(), Box<dyn std::error::Error>> {
        let folder = tempfile::tempdir()?;
        let mut command = CommandBuilder::new("/bin/sh");
        command.arg("-i");
        command.cwd(folder.path());
        command.env("PS1", "");
        command.env_remove("ENV");
        let session = Session::start(
            command,
            PtySize {
                rows: 8,
                cols: 60,
                ..PtySize::default()
            },
        )?;
        let wait_for = |predicate: &dyn Fn(&Frame) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(6);
            loop {
                let frame = session.output.borrow();
                if predicate(&frame) {
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "terminal state: {} {:?}",
                    frame.status,
                    frame.screen.text()
                );
                drop(frame);
                thread::sleep(Duration::from_millis(12));
            }
        };
        wait_for(&|frame| frame.running);
        session.send(Input::Resize(PtySize {
            rows: 11,
            cols: 57,
            ..PtySize::default()
        }))?;
        session.send(Input::Text(b"stty -echo\r".to_vec()))?;
        session.send(Input::Text(
            b"if [ -t 0 ] && [ -t 1 ]; then printf '%s%s\\n' TTY -OK; fi; stty size; basename \"$(pwd -P)\"\r"
                .to_vec(),
        ))?;
        let cwd = folder.path().canonicalize()?;
        let id = cwd
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("invalid fixture path")?;
        wait_for(&|frame| {
            let screen = frame.screen.text();
            screen.contains("TTY-OK") && screen.contains("11 57") && screen.contains(id)
        });
        session.send(Input::Text(
            b"printf '%s%s\\n' SLEEP -READY; sleep 30\r".to_vec(),
        ))?;
        wait_for(&|frame| frame.screen.text().contains("SLEEP-READY"));
        session.send(Input::Text(vec![3]))?;
        session.send(Input::Text(b"printf '%s%s\\n' SIGNAL -OK\r".to_vec()))?;
        wait_for(&|frame| frame.screen.text().contains("SIGNAL-OK"));
        session.send(Input::Text(b"exit 0\r".to_vec()))?;
        wait_for(&|frame| !frame.running && frame.status.contains("终端已"));
        Ok(())
    }
}
