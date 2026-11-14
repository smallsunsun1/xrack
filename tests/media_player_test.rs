use std::{path::Path, time::Instant};
use xrack_layout::{Interaction, Point, Size, TextMeasurer, layout};
use xrack_runtime::{
    Component, MediaCommand, MediaOptions, MediaQueue, MediaSession, MediaState, Value,
};

struct Playback(MediaState);

impl MediaSession for Playback {
    fn command(&mut self, command: MediaCommand) {
        match command {
            MediaCommand::Play => {
                self.0.playing = true;
                self.0.status = xrack_runtime::MediaStatus::Playing;
            }
            MediaCommand::Pause => self.0.playing = false,
            MediaCommand::Stop => {
                self.0.playing = false;
                self.0.position = 0.0;
                self.0.status = xrack_runtime::MediaStatus::Stopped;
            }
            MediaCommand::Seek(seconds) => {
                self.0.position = seconds.clamp(self.0.seek_start, self.0.seek_end);
                self.0.at_live_edge = false;
            }
            MediaCommand::GoLive => {
                if self.0.live {
                    self.0.position = self.0.seek_end - 2.0;
                    self.0.playing = true;
                    self.0.at_live_edge = true;
                }
            }
        }
    }

    fn set_volume(&mut self, volume: f64) {
        self.0.error = format!("volume={volume}");
    }

    fn configure(&mut self, options: &MediaOptions) {
        self.0.bitrate = options.network_bandwidth as i64;
    }

    fn sample(&mut self, _: Instant) -> MediaState {
        self.0.clone()
    }
    fn active(&self) -> bool {
        self.0.playing
    }
}

fn open(path: &Path, autoplay: bool, _: f64) -> Result<Box<dyn MediaSession>, String> {
    if path == Path::new("http://example.invalid/live.m3u8") {
        return Ok(Box::new(Playback(MediaState {
            playing: autoplay,
            ready: true,
            seekable: true,
            live: true,
            position: 18.0,
            seek_start: 10.0,
            seek_end: 20.0,
            at_live_edge: true,
            ..Default::default()
        })));
    }
    assert!(
        path.ends_with("crates/xrack-media/tests/sync.mp4"),
        "{path:?}"
    );
    Ok(Box::new(Playback(MediaState {
        playing: autoplay,
        ready: true,
        seekable: true,
        duration: 3.0,
        seek_end: 3.0,
        ..Default::default()
    })))
}

fn tick(queue: &mut MediaQueue, component: &impl Component) {
    let scene = component.render_shared();
    let player = scene.find("player").unwrap();
    queue.sync(player.media.as_ref());
    queue.advance(Instant::now());
}

struct Text;
impl TextMeasurer for Text {
    fn measure(&mut self, text: &str, _: f32, width: f32) -> Size {
        Size::new((text.chars().count() as f32 * 7.0).min(width), 16.0)
    }
}

fn drag_progress(component: &mut impl Component) {
    let scene = component.render_shared();
    let frame = layout(&scene, Size::new(720.0, 480.0), &mut Text);
    let progress = scene.find("progress").unwrap();
    let bounds = frame.at_path(&[0, 1]).unwrap().bounds;
    let geometry = xrack_layout::SliderGeometry::for_node(bounds, progress);
    let mut input = Interaction::default();
    assert!(input.range_pointer_press(&scene, &frame, geometry.position(0.0)));
    assert!(input.slider_press(&scene, &frame).is_none());
    let Point { x, y } = geometry.position(0.6);
    input.pointer_move(&scene, &frame, Point { x, y });
    assert!(input.slider_drag(&scene, &frame).is_none());
    let release = input.slider_release(&scene, &frame).unwrap();
    assert!(component.adjust_range_at(&scene, &release.path, release.action));
}

#[test]
fn media_scene_and_commands_agree_in_aot_and_interpreter() {
    xrack_runtime::register_media_backend(open);
    let source = include_str!("../examples/video/video.xr");
    let document = xrack_compiler::parse(source).unwrap();
    let module = xrack_compiler::lower(&document, "examples/video/video.xr").unwrap();
    let interpreted = xrack_interpreter::load_mlir(&module.to_mlir())
        .unwrap()
        .create("VideoDemo")
        .unwrap();
    let mut compiled = video_ui::VideoDemo::new();
    let mut aot_queue = MediaQueue::default();
    let mut interpreter_queue = MediaQueue::default();
    tick(&mut aot_queue, &compiled);
    tick(&mut interpreter_queue, &interpreted);
    assert!(compiled.get_ready());
    assert_eq!(interpreted.get("ready").unwrap(), Value::Bool(true));
    compiled.invoke_play();
    interpreted.invoke("play", &[]).unwrap();
    drag_progress(&mut compiled);
    let mut interpreted = interpreted;
    drag_progress(&mut interpreted);
    tick(&mut aot_queue, &compiled);
    tick(&mut interpreter_queue, &interpreted);
    assert!(compiled.get_playing());
    assert_eq!(interpreted.get("playing").unwrap(), Value::Bool(true));
    assert!(
        (compiled.get_position() - 1.8).abs() < 0.06,
        "{}",
        compiled.get_position()
    );
    assert_eq!(
        interpreted.get("position").unwrap(),
        Value::Float(compiled.get_position())
    );
    compiled.invoke_pause();
    interpreted.invoke("pause", &[]).unwrap();
    compiled.set_sound_level(0.35);
    interpreted.set("sound-level", Value::Float(0.35)).unwrap();
    tick(&mut aot_queue, &compiled);
    tick(&mut interpreter_queue, &interpreted);
    assert!(!compiled.get_playing());
    assert_eq!(interpreted.get("playing").unwrap(), Value::Bool(false));
    assert_eq!(compiled.get_error(), "volume=0.35");
    assert_eq!(
        interpreted.get("error").unwrap(),
        Value::String(compiled.get_error())
    );
    compiled.invoke_stop();
    interpreted.invoke("stop", &[]).unwrap();
    tick(&mut aot_queue, &compiled);
    tick(&mut interpreter_queue, &interpreted);
    assert_eq!(compiled.get_status(), "stopped");
    assert_eq!(
        interpreted.get("status").unwrap(),
        Value::String("stopped".into())
    );

    let live = "http://example.invalid/live.m3u8";
    compiled.set_video_source(live.into());
    interpreted
        .set("video-source", Value::String(live.into()))
        .unwrap();
    tick(&mut aot_queue, &compiled);
    tick(&mut interpreter_queue, &interpreted);
    assert!(compiled.get_is_live() && compiled.get_at_live_edge());
    assert_eq!(compiled.get_seek_start(), 10.0);
    assert_eq!(compiled.get_seek_end(), 20.0);
    assert_eq!(interpreted.get("is-live").unwrap(), Value::Bool(true));
    assert_eq!(interpreted.get("seek-start").unwrap(), Value::Float(10.0));
    assert_eq!(interpreted.get("seek-end").unwrap(), Value::Float(20.0));
    drag_progress(&mut compiled);
    drag_progress(&mut interpreted);
    tick(&mut aot_queue, &compiled);
    tick(&mut interpreter_queue, &interpreted);
    assert!((compiled.get_position() - 16.0).abs() < 0.06);
    assert!(!compiled.get_at_live_edge());
    assert_eq!(
        interpreted.get("position").unwrap(),
        Value::Float(compiled.get_position())
    );
    assert_eq!(interpreted.get("at-live-edge").unwrap(), Value::Bool(false));
    compiled.invoke_go_live();
    interpreted.invoke("go-live", &[]).unwrap();
    tick(&mut aot_queue, &compiled);
    tick(&mut interpreter_queue, &interpreted);
    assert!(compiled.get_playing() && compiled.get_at_live_edge());
    assert_eq!(compiled.get_position(), 18.0);
    assert_eq!(interpreted.get("position").unwrap(), Value::Float(18.0));
    assert_eq!(interpreted.get("at-live-edge").unwrap(), Value::Bool(true));

    compiled.set_network_controls(true);
    interpreted
        .set("network-controls", Value::Bool(true))
        .unwrap();
    for (id, bandwidth) in [
        ("weak-network-button", 180_000),
        ("restore-network-button", 0),
    ] {
        assert!(compiled.dispatch(id, "clicked"));
        assert!(interpreted.dispatch(id, "clicked"));
        tick(&mut aot_queue, &compiled);
        tick(&mut interpreter_queue, &interpreted);
        assert_eq!(compiled.get_bitrate(), bandwidth);
        assert_eq!(interpreted.get("bitrate").unwrap(), Value::Int(bandwidth));
        assert!(compiled.get_playing());
        assert_eq!(compiled.get_position(), 18.0);
    }
}

#[test]
fn standalone_mlir_resolves_media_sources_and_subtitles_without_locations() {
    use std::{fs, path::PathBuf};
    use xrack_runtime::MediaOptions;

    struct CaptureSource(PathBuf, MediaOptions);

    impl MediaSession for CaptureSource {
        fn command(&mut self, _: MediaCommand) {}
        fn set_volume(&mut self, _: f64) {}
        fn configure(&mut self, options: &MediaOptions) {
            self.1 = options.clone();
        }
        fn sample(&mut self, _: Instant) -> MediaState {
            MediaState {
                title: self.0.to_string_lossy().into_owned(),
                subtitle: self
                    .1
                    .subtitle_file
                    .as_ref()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                ..MediaState::default()
            }
        }
        fn active(&self) -> bool {
            false
        }
    }

    xrack_runtime::register_media_backend(|path, _, _| {
        Ok(Box::new(CaptureSource(
            path.to_owned(),
            MediaOptions::default(),
        )))
    });
    let source = r#"export component App inherits Window {
        in property <string> source: "assets/clip.mp4";
        out property <string> opened-source: player.media-title;
        out property <string> opened-subtitle: player.subtitle;
        player := MediaPlayer {
            source: root.source;
            subtitle-file: "assets/captions.vtt";
        }
    }"#;
    let directory = std::env::var_os("TEST_TMPDIR")
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join(format!("xrack-media-locations-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let file = directory.join("view.mlir");
    let declaration = directory.join("original/view.xr");
    let document = xrack_compiler::parse(source).unwrap();
    let module = xrack_compiler::lower(&document, declaration.to_str().unwrap()).unwrap();
    let with_locations = module.to_mlir();
    let without_locations = with_locations
        .lines()
        .filter(|line| !line.starts_with("#loc"))
        .map(|line| line.split_once(" loc(").map_or(line, |(op, _)| op))
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(with_locations, without_locations);
    for (text, entry) in [(&with_locations, &declaration), (&without_locations, &file)] {
        fs::write(&file, text).unwrap();
        let module = xrack_compiler::mlir::process(text).unwrap();
        let rust = xrack_compiler::generate_rust(&module, file.to_str().unwrap(), None)
            .unwrap()
            .to_string();
        let expected = format!("{:?}", entry.to_str().unwrap());
        let population = rust.split(". populate").nth(1).unwrap_or_default();
        let arguments = population.split(')').next().unwrap_or_default();
        assert!(
            arguments.contains(&expected),
            "expected {expected} in MediaSlot::populate{arguments}"
        );

        let ui = xrack_interpreter::load_mlir_file(&file)
            .unwrap()
            .create("App")
            .unwrap();
        let mut queue = MediaQueue::default();
        let assert_opened = |property, path: &Path| {
            assert_eq!(
                ui.get(property).unwrap(),
                Value::String(path.to_string_lossy().into_owned())
            );
        };
        let base = entry.parent().unwrap();
        tick(&mut queue, &ui);
        assert_opened("opened-source", &base.join("assets/clip.mp4"));
        assert_opened("opened-subtitle", &base.join("assets/captions.vtt"));

        let absolute = directory.join("absolute.mp4");
        for uri in [
            "https://example.invalid/clip.mp4",
            absolute.to_str().unwrap(),
        ] {
            ui.set("source", uri.to_owned().into()).unwrap();
            tick(&mut queue, &ui);
            assert_opened("opened-source", Path::new(uri));
            assert_opened("opened-subtitle", &base.join("assets/captions.vtt"));
        }
        assert_eq!(ui.take_error(), None);
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn state_metrics_are_read_only() {
    let document =
        xrack_compiler::parse("component App inherits Window { MediaPlayer { duration: 2; } }")
            .unwrap();
    let failure = xrack_compiler::lower(&document, "invalid-video.xr").unwrap_err();
    assert!(failure.to_string().contains("read-only"));
}

#[test]
fn repeated_media_requires_a_component_for_both_source_and_text_mlir() {
    let source = r#"
        export component App inherits Window {
            for index in 2: VerticalLayout { Rectangle {} }
        }
    "#;
    let file = "repeated-media.xr";
    let expected = "MediaPlayer in a repetition requires a component delegate";
    let repeated = source.replace("Rectangle", "MediaPlayer");
    let document = xrack_compiler::parse(&repeated).unwrap();
    let error = xrack_compiler::lower(&document, file).unwrap_err();
    assert!(error.to_string().contains(expected), "{error}");

    let document = xrack_compiler::parse(source).unwrap();
    let raw = xrack_compiler::mlir::lower(&document, file).unwrap();
    let malformed = raw.replace("kind = \"Rectangle\"", "kind = \"MediaPlayer\"");
    assert_ne!(raw, malformed);
    let error = xrack_compiler::mlir::Module::parse(&malformed).unwrap_err();
    assert!(error.contains(expected), "{error}");
    assert_eq!(
        xrack_interpreter::load_mlir(&malformed).err(),
        Some(xrack_interpreter::Error::Invalid(error))
    );

    let delegated = r#"
        component Clip inherits MediaPlayer {}
        export component App inherits Window {
            ListView {
                row-height: 30px;
                for index in 2: Clip {}
            }
        }
    "#;
    let document = xrack_compiler::parse(delegated).unwrap();
    let module = xrack_compiler::lower(&document, file).unwrap();
    assert!(xrack_compiler::codegen::generate(&module).is_ok());
    let program = xrack_interpreter::load_mlir(&module.to_mlir()).unwrap();
    let app = program.create("App").unwrap();
    let rows = app.list_rows(&[0], 0..2);
    assert_eq!(rows.len(), 2);
    let first = rows[0].media.as_ref().unwrap();
    let second = rows[1].media.as_ref().unwrap();
    assert_ne!(first, second);
}

#[cfg(feature = "media_backend")]
#[test]
fn generated_and_interpreted_video_rewind_real_hls() {
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::Duration,
    };

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let shutdown = stop.clone();
    let server = thread::spawn(move || {
        let playlist = include_str!("../crates/xrack-media/tests/hls/vod.m3u8")
            .replace("#EXT-X-PLAYLIST-TYPE:VOD", "#EXT-X-PLAYLIST-TYPE:EVENT")
            .replace("#EXT-X-ENDLIST\n", "");
        while !shutdown.load(Ordering::Relaxed) {
            let Ok((mut connection, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(2));
                continue;
            };
            connection
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut request = String::new();
            if BufReader::new(&connection).read_line(&mut request).is_err() {
                continue;
            }
            let (mime, body): (&str, &[u8]) = match request.split_whitespace().nth(1) {
                Some("/test.m3u8") => ("application/vnd.apple.mpegurl", playlist.as_bytes()),
                Some("/segment0.ts") => (
                    "video/mp2t",
                    include_bytes!("../crates/xrack-media/tests/hls/segment0.ts"),
                ),
                Some("/segment1.ts") => (
                    "video/mp2t",
                    include_bytes!("../crates/xrack-media/tests/hls/segment1.ts"),
                ),
                Some("/segment2.ts") => (
                    "video/mp2t",
                    include_bytes!("../crates/xrack-media/tests/hls/segment2.ts"),
                ),
                _ => {
                    let _ = connection.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    continue;
                }
            };
            let _ = write!(
                connection,
                "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .and_then(|_| connection.write_all(body));
        }
    });

    fn wait(queue: &mut MediaQueue, component: &impl Component, test: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            tick(queue, component);
            if test() {
                return;
            }
            assert!(Instant::now() < deadline, "media predicate timed out");
            thread::sleep(Duration::from_millis(10));
        }
    }

    let result = std::panic::catch_unwind(|| {
        xrack_media::install();
        let source = include_str!("../examples/video/video.xr");
        let document = xrack_compiler::parse(source).unwrap();
        let module = xrack_compiler::lower(&document, "examples/video/video.xr").unwrap();
        let mut interpreted = xrack_interpreter::load_mlir(&module.to_mlir())
            .unwrap()
            .create("VideoDemo")
            .unwrap();
        let mut compiled = video_ui::VideoDemo::new();
        let url = format!("http://{address}/test.m3u8");
        compiled.set_video_source(url.clone());
        interpreted.set("video-source", Value::String(url)).unwrap();
        let mut aot_queue = MediaQueue::default();
        let mut interpreter_queue = MediaQueue::default();

        wait(&mut aot_queue, &compiled, || compiled.get_ready());
        wait(&mut interpreter_queue, &interpreted, || {
            interpreted.get("ready").unwrap() == Value::Bool(true)
        });
        assert!(compiled.get_is_live() && compiled.get_seekable());
        assert!((compiled.get_seek_end() - compiled.get_seek_start() - 3.0).abs() < 0.1);
        assert_eq!(interpreted.get("is-live").unwrap(), Value::Bool(true));
        drag_progress(&mut compiled);
        drag_progress(&mut interpreted);
        wait(&mut aot_queue, &compiled, || {
            compiled.get_ready() && compiled.get_position() > 1.65
        });
        wait(&mut interpreter_queue, &interpreted, || {
            interpreted.get("ready").unwrap() == Value::Bool(true)
                && matches!(interpreted.get("position").unwrap(), Value::Float(position) if position > 1.65)
        });
        assert!((compiled.get_position() - 1.8).abs() < 0.1);
        assert!(
            compiled
                .render_shared()
                .find("player")
                .unwrap()
                .media_frame
                .is_some()
        );
        assert!(
            interpreted
                .render_shared()
                .find("player")
                .unwrap()
                .media_frame
                .is_some()
        );

        compiled.invoke_go_live();
        interpreted.invoke("go-live", &[]).unwrap();
        wait(&mut aot_queue, &compiled, || {
            compiled.get_playing() && compiled.get_at_live_edge() && compiled.get_position() < 0.6
        });
        wait(&mut interpreter_queue, &interpreted, || {
            interpreted.get("at-live-edge").unwrap() == Value::Bool(true)
                && interpreted.get("playing").unwrap() == Value::Bool(true)
                && matches!(interpreted.get("position").unwrap(), Value::Float(position) if position < 0.6)
        });
    });
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
