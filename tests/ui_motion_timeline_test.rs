#![cfg(test)]

use layout_ui::AnimationsDemo;
use serde::Serialize;
use serde_json::Value;
use std::time::{Duration, Instant};
use trace_capture::{Capture, events_json};
use tracing_subscriber::{filter::filter_fn, prelude::*};
use xrack_interpreter::Program;
use xrack_layout::{Interaction, Size, layout};
use xrack_renderer_femtovg::Renderer;
use xrack_runtime::{
    Component, Node, SceneTransition,
    ui_trace::{Buffer, TARGET},
    update_profile,
};
use xrack_testing::FixedText;

const SIZE: [u32; 2] = [546, 288];

#[derive(Debug, PartialEq, Serialize)]
struct Frame {
    ms: u64,
    linear: [f32; 4],
    eased: [f32; 4],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    png: Vec<u8>,
    #[serde(skip_serializing_if = "Value::is_null")]
    events: Value,
}

fn sample(start: &Node, end: &Node, mut report: Option<(&mut Renderer, &Buffer)>) -> Vec<Frame> {
    let paths = ["线性进度条", "缓动进度条"].map(|name| {
        [start
            .children
            .iter()
            .position(|node| node.text("accessible-name") == Some(name))
            .unwrap_or_else(|| panic!("missing {name} under {}", start.kind))]
    });
    let transition = SceneTransition::between(start, end);
    let mut frames = Vec::new();
    let input = Interaction::default();
    let viewport = Size::new(SIZE[0] as f32, SIZE[1] as f32);
    for ms in (0..=10_000).step_by(10) {
        let elapsed = Duration::from_millis(ms);
        let mut png = Vec::new();
        let (measured, events) = if let Some((renderer, buffer)) = &mut report {
            let span = tracing::debug_span!(target: TARGET, "motion_frame", animation_ms = ms);
            let layout = span.in_scope(|| {
                let start = Instant::now();
                let scene = transition.frame(end, elapsed);
                tracing::debug!(target: TARGET, event = "motion_interpolated", wall_micros = micros(start));
                let mut prepared = None;
                update_profile::measure(|| {
                    update_profile::layout(false);
                    prepared = Some(renderer.layout_with_input(&scene, viewport, &input));
                });
                let start = Instant::now();
                let snapshot = renderer
                    .snapshot_prepared(&scene, prepared.unwrap(), SIZE[0], SIZE[1], 1.0, &input)
                    .unwrap();
                tracing::debug!(target: TARGET, event = "motion_render_readback", wall_micros = micros(start));
                let start = Instant::now();
                snapshot.write_png(&mut png).unwrap();
                tracing::debug!(target: TARGET, event = "motion_png_encoded", wall_micros = micros(start));
                snapshot.layout
            });
            (layout, events_json(buffer.take()))
        } else {
            let scene = transition.frame(end, elapsed);
            (layout(&scene, viewport, &mut FixedText), Value::Null)
        };
        let [linear, eased] = paths.map(|path| {
            let bounds = measured.at_path(&path).unwrap().bounds;
            [bounds.x, bounds.y, bounds.width, bounds.height]
        });
        frames.push(Frame {
            ms,
            linear,
            eased,
            png,
            events,
        });
        if !transition.running(elapsed) {
            return frames;
        }
    }
    panic!("motion report supports transitions up to ten seconds");
}

fn micros(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
}

#[test]
fn sample_animation_timeline() {
    let mut ui = AnimationsDemo::new();
    let base = ui.render_shared();
    ui.set_expanded(true);
    let compiled = sample(&base, &ui.render_shared(), None);

    let source = include_str!("../examples/layout/layout.xr");
    let preview = xrack_compiler::prepare_preview(source, "layout.xr", Default::default()).unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    let ui = program.create("AnimationsDemo").unwrap();
    let base = ui.render_shared();
    ui.set("expanded", true.into()).unwrap();
    let expanded = ui.render_shared();
    assert_eq!(sample(&base, &expanded, None), compiled);
    assert!(
        compiled.len() > 2,
        "the demo must contain an animated interval"
    );

    if let Some(path) = std::env::var_os("XRACK_MOTION_REPORT") {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let mut renderer = pollster::block_on(Renderer::new(instance, None)).unwrap();
        let buffer = Buffer::default();
        let subscriber = tracing_subscriber::registry().with(
            Capture(buffer.clone()).with_filter(filter_fn(|metadata| metadata.target() == TARGET)),
        );
        let frames = tracing::subscriber::with_default(subscriber, || {
            sample(&base, &expanded, Some((&mut renderer, &buffer)))
        });
        for (drawn, layout) in frames.iter().zip(&compiled) {
            assert_eq!((drawn.linear, drawn.eased), (layout.linear, layout.eased));
        }
        let trace = serde_json::to_string(&frames).unwrap();
        let report =
            include_str!("ui_motion_report.html").replacen("<!--XRACK_MOTION-->", &trace, 1);
        std::fs::write(path, report).unwrap();
    }
}
