use std::{path::Path, time::Instant};
use xrack_layout::{Size, TextMeasurer, WindowEffects, layout};
use xrack_runtime::{Component, MediaCommand, MediaQueue, MediaSession, MediaState, MediaStatus};

const SOURCE: &str = "examples/sound_effect/sound_effect.xr";

struct Text;

impl TextMeasurer for Text {
    fn measure(&mut self, text: &str, _: f32, width: f32) -> Size {
        Size::new((text.chars().count() as f32 * 7.0).min(width), 16.0)
    }
}

fn tick(ui: &impl Component) {
    let scene = ui.render_shared();
    assert!(scene.find("confirmation").is_none());
    let layout = layout(&scene, Size::new(400.0, 310.0), &mut Text);
    let effects = WindowEffects::collect(&scene, &layout);
    assert_eq!(effects.media.len(), 1);
    let mut queue = MediaQueue::default();
    queue.sync(effects.media);
    queue.advance(Instant::now());
}

#[test]
fn sound_effect_is_nonvisual_and_uses_declaration_directory() {
    struct Loaded;

    impl MediaSession for Loaded {
        fn command(&mut self, _: MediaCommand) {}
        fn set_volume(&mut self, _: f64) {}
        fn sample(&mut self, _: Instant) -> MediaState {
            MediaState {
                ready: true,
                status: MediaStatus::Ready,
                ..MediaState::default()
            }
        }
        fn active(&self) -> bool {
            false
        }
    }

    xrack_runtime::register_sound_backend(|path, volume| {
        assert!(
            path.ends_with(Path::new("examples/sound_effect/click.wav")),
            "{path:?}"
        );
        assert_eq!(volume, 0.35);
        Ok(Box::new(Loaded))
    });

    let aot = sound_ui::SoundDemo::new();
    tick(&aot);
    assert!(aot.get_sound_ready());
    assert_eq!(aot.get_sound_status(), "ready");

    let document = xrack_compiler::parse(&std::fs::read_to_string(SOURCE).unwrap()).unwrap();
    let module = xrack_compiler::lower(&document, SOURCE).unwrap();
    let interpreted = xrack_interpreter::load_mlir(&module.to_mlir())
        .unwrap()
        .create("SoundDemo")
        .unwrap();
    tick(&interpreted);
    assert_eq!(
        interpreted.get("sound-status").unwrap(),
        xrack_runtime::Value::String("ready".into())
    );
}

#[test]
fn sound_effect_rejects_visual_bindings_and_video_commands() {
    let source = std::fs::read_to_string(SOURCE).unwrap();
    for (from, to, expected) in [
        ("volume <- 0.35;", "width <- 10px;", "width"),
        ("volume <- 0.35;", "ready <- true;", "read-only"),
        ("confirmation.play();", "confirmation.pause();", "pause"),
        (
            "confirmation.play();",
            "confirmation.play(1);",
            "takes no arguments",
        ),
    ] {
        assert!(source.contains(from), "missing fixture statement: {from}");
        let document = xrack_compiler::parse(&source.replacen(from, to, 1)).unwrap();
        let error = xrack_compiler::lower(&document, SOURCE).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }

    let output = Path::new(SOURCE).with_extension("mlir");
    let text = xrack_compiler::export_mlir_to(
        &xrack_compiler::parse(&source).unwrap(),
        SOURCE,
        None,
        &output,
    )
    .unwrap();
    assert!(
        text.contains("xr.media_source_file = \"sound_effect.xr\""),
        "{text}"
    );
    let invalid = text.replacen("method = \"play\"", "method = \"pause\"", 1);
    let error = xrack_compiler::mlir::process(&invalid).unwrap_err();
    assert!(
        error.contains("requires a media owner supporting this command"),
        "{error}"
    );
}
