mod code_review;
mod context;
mod drafts;
mod followup;
mod models;
mod project_index;
mod prompt;
mod protocol;
mod questions;
mod skills;
mod tab_store;
mod thread_index;
mod turn_diff;

pub use code_review::ReviewTarget;
pub use context::TokenCounts;
pub use drafts::{DraftContent, DraftKey, DraftStore, SavedDraft, SavedReviewNote};
pub use followup::QueuedMessage;
pub use prompt::{
    Attachment, AttachmentContent, ExcerptOrigin, FileExcerpt, Prompt, TerminalOutput,
};
use protocol::Protocol;
pub use protocol::{
    ActivityState, CommandActivity, ConversationActivity, ConversationSummary, Entry, ForkPoint,
    Input, Snapshot, Status,
};
use serde_json::Value;
pub use skills::{Skill, SkillCatalog, SkillCatalogState};
use std::{
    cell::Cell,
    ffi::OsStr,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{RecvTimeoutError, SyncSender, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio::sync::watch;
pub use turn_diff::TurnDiff;

const FRAME: Duration = Duration::from_millis(16);
const MAX_FRAME_BYTES: u64 = 16 * 1024 * 1024;

enum Mail {
    Input(u64, Input),
    Frame(Value),
    Closed(String),
}

#[derive(Clone)]
pub struct Commands {
    tx: SyncSender<Mail>,
    sent: Rc<Cell<u64>>,
}

impl Commands {
    pub fn send(&self, input: Input) -> Result<(), String> {
        let next = self.sent.get() + 1;
        self.tx
            .try_send(Mail::Input(next, input))
            .map_err(|error| match error {
                TrySendError::Full(_) => "连接繁忙，请稍后再试".to_owned(),
                TrySendError::Disconnected(_) => "Codex 连接已关闭，请重启工作台".to_owned(),
            })?;
        self.sent.set(next);
        Ok(())
    }

    pub fn is_current(&self, snapshot: &Snapshot) -> bool {
        !snapshot.connected || snapshot.acknowledged >= self.sent.get()
    }
}

pub struct Client {
    commands: Commands,
    closed: Arc<AtomicBool>,
    child: Arc<Mutex<Option<ChildProcess>>>,
    worker: Option<JoinHandle<()>>,
}

impl Client {
    pub fn start(
        executable: &Path,
        workspace: String,
        workspace_write: bool,
        state_directory: Option<PathBuf>,
        keep_launch_workspace: bool,
    ) -> std::io::Result<(Self, watch::Receiver<Snapshot>)> {
        let (tx, inbox) = sync_channel(64);
        let (view, output) = watch::channel(Snapshot::connecting(workspace.clone()));
        let closed = Arc::new(AtomicBool::new(false));
        let worker_closed = closed.clone();
        let child = Arc::new(Mutex::new(None));
        let worker_child = child.clone();
        let reader_mailbox = tx.clone();
        let executable = executable.to_owned();
        let worker = thread::Builder::new()
            .name("xrack-agent-session".into())
            .spawn(move || {
                let fork_index = state_directory
                    .as_ref()
                    .map(|directory| directory.join("forks"));
                let mut protocol = Protocol::new(workspace, workspace_write, fork_index);
                protocol.restore_workspace_state(state_directory, keep_launch_workspace);
                let result = run(
                    executable.as_os_str(),
                    reader_mailbox,
                    &inbox,
                    &view,
                    &worker_closed,
                    &worker_child,
                    &mut protocol,
                );
                if worker_closed.load(Ordering::Relaxed) {
                    for mail in inbox.try_iter() {
                        if let Mail::Input(_, input @ Input::SaveOpenTabs { .. }) = mail {
                            protocol.input(input);
                        }
                    }
                }
                let process = worker_child
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                drop(process);
                if let Err(error) = result {
                    protocol.disconnected(error);
                    if let Some(snapshot) = protocol.take_snapshot() {
                        view.send_replace(snapshot);
                    }
                }
            })?;
        Ok((
            Self {
                commands: Commands {
                    tx,
                    sent: Rc::new(Cell::new(0)),
                },
                closed,
                child,
                worker: Some(worker),
            },
            output,
        ))
    }

    pub fn commands(&self) -> Commands {
        self.commands.clone()
    }

    /// Join after the UI event loop has finished.
    pub fn shutdown(mut self) {
        self.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
    fn stop(&self) {
        self.closed.store(true, Ordering::Relaxed);
        if let Some(process) = self
            .child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        {
            // Closing the child also releases a writer blocked on the stdin pipe.
            let _ = process.0.kill();
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stop();
    }
}

struct ChildProcess(Child);

impl Drop for ChildProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run(
    executable: &OsStr,
    mailbox: SyncSender<Mail>,
    inbox: &std::sync::mpsc::Receiver<Mail>,
    view: &watch::Sender<Snapshot>,
    closed: &AtomicBool,
    process: &Mutex<Option<ChildProcess>>,
    protocol: &mut Protocol,
) -> Result<(), String> {
    let mut child = ChildProcess(
        Command::new(executable)
            .args(["app-server", "--listen", "stdio://"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| format!("启动 Codex 失败：{error}"))?,
    );
    let mut writer = child.0.stdin.take().ok_or("无法打开 Codex 输入")?;
    let reader = child.0.stdout.take().ok_or("无法打开 Codex 输出")?;
    *process
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(child);
    thread::Builder::new()
        .name("xrack-agent-stdout".into())
        .spawn(move || read_frames(BufReader::new(reader), &mailbox))
        .map_err(|error| format!("启动 Codex 读取线程失败：{error}"))?;

    let mut last_publish = Instant::now();
    while !closed.load(Ordering::Relaxed) {
        for message in protocol.drain_outbound() {
            serde_json::to_writer(&mut writer, &message)
                .map_err(|error| format!("写入 Codex 请求失败：{error}"))?;
            writer
                .write_all(b"\n")
                .map_err(|error| format!("写入 Codex 请求失败：{error}"))?;
        }
        if last_publish.elapsed() >= FRAME {
            if let Some(snapshot) = protocol.take_snapshot() {
                view.send_replace(snapshot);
            }
            last_publish = Instant::now();
        }
        if protocol.expired() {
            return Err("等待 Codex 协议响应超时".into());
        }
        match inbox.recv_timeout(FRAME) {
            Ok(Mail::Input(sequence, input)) => {
                protocol.input(input);
                protocol.acknowledge(sequence);
            }
            Ok(Mail::Frame(message)) => protocol.server(message)?,
            Ok(Mail::Closed(reason)) => return Err(reason),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

fn read_frames(mut reader: impl BufRead, mailbox: &SyncSender<Mail>) {
    let reason = read_messages(&mut reader, |frame| {
        mailbox.send(Mail::Frame(frame)).is_ok()
    })
    .err()
    .unwrap_or_else(|| "Codex 进程已退出".to_owned());
    let _ = mailbox.send(Mail::Closed(reason));
}

fn read_messages(
    reader: &mut impl BufRead,
    mut on_message: impl FnMut(Value) -> bool,
) -> Result<(), String> {
    let mut line = Vec::new();
    loop {
        line.clear();
        let length = reader
            .take(MAX_FRAME_BYTES + 1)
            .read_until(b'\n', &mut line)
            .map_err(|error| format!("读取 Codex 响应失败：{error}"))?;
        if length == 0 {
            return Ok(());
        }
        if length as u64 > MAX_FRAME_BYTES {
            return Err("Codex 响应超过 JSON 消息大小限制".into());
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let frame = serde_json::from_slice(&line)
            .map_err(|error| format!("无法解析 Codex 消息：{error}"))?;
        if !on_message(frame) {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_utf8_json_lines_and_reports_bad_frames() {
        let mut wire = "\n{\"id\":1}\r\n{\"delta\":\"中文\"}\ninvalid json\n".as_bytes();
        let mut messages = Vec::new();
        let result = read_messages(&mut wire, |frame| {
            messages.push(frame);
            true
        });
        assert!(result.unwrap_err().contains("无法解析"));
        assert_eq!(messages, [json!({"id":1}), json!({"delta":"中文"})]);
    }
    #[cfg(unix)]
    #[test]
    fn child_process_streams_and_interrupts_over_stdio() {
        use crate::project_index::ProjectIndex;
        use crate::tab_store::TabStore;
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("codex-fixture");
        std::fs::write(&executable, r#"#!/bin/sh
set -eu
receive() {
    IFS= read -r message
    case "$message" in
        *"\"method\":\"$1\""*) ;;
        *) exit 1 ;;
    esac
}
receive initialize
printf '%s
' '{"id":1,"result":{}}'
receive initialized
receive thread/list
printf '%s
' '{"id":2,"result":{"data":[]}}'
receive model/list
printf '%s
' '{"id":3,"result":{"data":[]}}'
receive collaborationMode/list
printf '%s
' '{"id":4,"result":{"data":[]}}'
receive thread/start
printf '%s
' '{"id":5,"result":{"thread":{"id":"thread"}}}'
receive turn/start
printf '%s
' '{"id":6,"result":{"turn":{"id":"turn"}}}'
printf '%s
' '{"method":"item/started","params":{"threadId":"thread","turnId":"turn","item":{"type":"commandExecution","id":"cmd","command":"ls examples","status":"inProgress"}}}'
printf '%s
' '{"method":"item/commandExecution/outputDelta","params":{"threadId":"thread","turnId":"turn","itemId":"cmd","delta":"counter"}}'
printf '%s
' '{"method":"item/agentMessage/delta","params":{"threadId":"thread","turnId":"turn","itemId":"answer","delta":"你好"}}'
receive turn/interrupt
printf '%s
' '{"id":7,"result":{}}'
printf '%s
' '{"method":"turn/completed","params":{"threadId":"thread","turn":{"id":"turn","status":"interrupted"}}}'
IFS= read -r _
"#).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let state_directory = directory.path().join("state");
        let workspace = directory
            .path()
            .join("current")
            .to_str()
            .unwrap()
            .to_owned();
        std::fs::create_dir_all(&workspace).unwrap();
        let previous_workspace = directory
            .path()
            .join("previous")
            .to_str()
            .unwrap()
            .to_owned();
        let index = ProjectIndex::new(Some(state_directory.clone()));
        index
            .remember(&previous_workspace, &mut Vec::new())
            .unwrap();
        let (client, output) = Client::start(
            &executable,
            workspace.clone(),
            false,
            Some(state_directory.clone()),
            true,
        )
        .unwrap();
        let commands = client.commands();
        let wait_for = |output: &watch::Receiver<Snapshot>, predicate: fn(&Snapshot) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let snapshot = output.borrow().clone();
                if predicate(&snapshot) {
                    return snapshot;
                }
                assert!(
                    Instant::now() < deadline,
                    "last status: {:?}",
                    snapshot.view.status
                );
                thread::sleep(Duration::from_millis(10));
            }
        };
        let initialized = wait_for(&output, |state| state.connected);
        assert_eq!(
            initialized.projects.as_ref(),
            &[workspace.clone(), previous_workspace.clone()]
        );
        commands.send(Input::Send("hello".into())).unwrap();
        let streaming = wait_for(&output, |state| state.view.entries.len() == 3);
        assert!(streaming.view.busy);
        assert!(
            matches!(streaming.view.entries[1].as_ref(), Entry::Command(command)
            if command.output == "counter" && command.state == ActivityState::Running)
        );
        assert!(
            matches!(streaming.view.entries[2].as_ref(), Entry::Assistant {text, ..} if text == "你好")
        );
        commands
            .send(Input::Interrupt(streaming.view.conversation))
            .unwrap();
        let stopped = wait_for(&output, |state| state.view.status == Status::Stopped);
        assert!(!stopped.view.busy);
        assert!(
            matches!(stopped.view.entries[1].as_ref(), Entry::Command(command)
            if command.state == ActivityState::Interrupted)
        );
        assert!(
            matches!(stopped.view.entries[2].as_ref(), Entry::Assistant {text, ..} if text == "你好")
        );
        commands
            .send(Input::SaveOpenTabs {
                open: vec![stopped.view.conversation],
                active: Some(stopped.view.conversation),
            })
            .unwrap();
        client.shutdown();
        assert_eq!(
            index.load().unwrap(),
            [workspace.clone(), previous_workspace]
        );
        let saved = TabStore::new(Some(state_directory.clone())).load().unwrap();
        assert_eq!(saved.open.len(), 1);
        assert_eq!(saved.active, Some(DraftKey::Thread("thread".into())));

        let mut reply = serde_json::to_vec(&json!({
            "id":5,"result":{"thread":{"id":"thread","cwd":workspace,"name":"从存档恢复"}}
        }))
        .unwrap();
        reply.push(b'\n');
        std::fs::write(executable.with_extension("read-reply"), reply).unwrap();
        std::fs::write(&executable, r#"#!/bin/sh
set -eu
receive() {
    IFS= read -r message
    case "$message" in
        *"\"method\":\"$1\""*) ;;
        *) exit 1 ;;
    esac
}
receive initialize
printf '%s\n' '{"id":1,"result":{}}'
receive initialized
receive thread/list
printf '%s\n' '{"id":2,"result":{"data":[]}}'
receive model/list
printf '%s\n' '{"id":3,"result":{"data":[]}}'
receive collaborationMode/list
printf '%s\n' '{"id":4,"result":{"data":[]}}'
receive thread/read
cat "$0.read-reply"
receive thread/turns/list
printf '%s\n' '{"id":6,"result":{"data":[{"id":"turn","status":"completed","items":[{"type":"userMessage","content":[{"type":"text","text":"hello"}]}]}]}}'
receive thread/queue/list
IFS= read -r _
"#).unwrap();
        let (reopened, output) =
            Client::start(&executable, workspace, false, Some(state_directory), false).unwrap();
        let restored = wait_for(&output, |state| {
            state.restored_tabs.is_some() && !state.view.restoring
        });
        assert_eq!(restored.view.draft_key, DraftKey::Thread("thread".into()));
        assert_eq!(
            restored.restored_tabs.as_deref().unwrap().as_slice(),
            [restored.view.conversation]
        );
        assert!(
            matches!(restored.view.entries[0].as_ref(), Entry::User(message) if message.prompt_text() == "hello")
        );
        reopened.shutdown();
    }
}
