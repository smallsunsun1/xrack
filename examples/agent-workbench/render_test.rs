mod file_references;

use std::fs::File;
use workbench_ui::{
    ActivityRow, AgentWorkbench, AttachmentRow, ChatFindDocument, CommandGroup, ConversationRow,
    ConversationTab, GenerationChoice, PaletteItem, ProjectGroupRow, QueuedRow, ReviewFile,
    ReviewLine, ReviewMatchRow, ReviewNoteRow, ReviewPair, SidebarMenuState, TerminalTab,
    TranscriptEntry, TranscriptSection, TurnActivity, WorkspaceFile, WorkspacePreviewTab,
};
use xrack_layout::{Interaction, Point, Size, ViewportObservers};
use xrack_renderer_femtovg::Renderer;
use xrack_runtime::{
    Color, ColorScheme, Component, Image, ImageFormat, Length, LoadedImage, LocaleContext,
    StyledLink, StyledRun, StyledText, TextStyle, TreeCommand,
};

fn transcript_benchmark_window() -> Result<AgentWorkbench, Box<dyn std::error::Error>> {
    let mut ui = AgentWorkbench::new();
    ui.set_connected(true);
    ui.set_workspace("/workspace/xrack".into());
    let entries = ui.model_entries();
    for index in 0..120 {
        let text = format!(
            "**第 {index} 条回复**\n\n渲染检查包括桌面窗口、文本布局、键盘输入和无障碍。\
             The transcript contains mixed Chinese and English text, **emphasis**, `cargo test`, \
             and a [repository link](https://example.invalid/review/{index}).\n\n\
             滚动经过这条消息时，只改变它在视口中的位置。第 {index} 条消息包含样式和链接。"
        );
        entries.push(TranscriptEntry {
            kind: "assistant".into(),
            heading: "Codex".into(),
            status: "已完成".into(),
            body: StyledText::from_markdown(&text)?,
            ..Default::default()
        });
    }
    Ok(ui)
}

#[test]
#[ignore = "run explicitly to measure rich-text layout with the native renderer"]
fn long_transcript_scroll_benchmark() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Instant;
    use xrack_runtime::Orientation;

    let viewport = Size::new(1120.0, 760.0);
    let mut ui = transcript_benchmark_window()?;
    let entries = ui.model_entries();
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut input = Interaction::default();
    let started = Instant::now();
    let (mut scene, mut layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let initial = started.elapsed().as_secs_f64() * 1000.0;
    fn scroll_offset(layout: &xrack_layout::LayoutNode) -> Option<f32> {
        layout
            .scroll_on(Orientation::Vertical)
            .filter(|area| area.max_offset > 1000.0)
            .map(|area| area.offset)
            .or_else(|| layout.children.iter().find_map(scroll_offset))
    }
    let mut observers = ViewportObservers::default();
    for (case, deliver_scroll) in [("layout", false), ("layout and scroll callbacks", true)] {
        let mut times = Vec::with_capacity(20);
        if deliver_scroll {
            input.clear_scroll_events();
            input.reconcile(&scene, &layout);
            observers.sync(&scene, &layout);
        }
        for index in 0..22 {
            input.pointer_move(&scene, &layout, Point { x: 650.0, y: 340.0 });
            let before = scroll_offset(&layout).ok_or("transcript has no scrollable extent")?;
            let start = Instant::now();
            assert!(input.scroll_wheel(&layout, Point { x: 0.0, y: 65.0 }, false));
            (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
            if deliver_scroll {
                input.reconcile(&scene, &layout);
                observers.sync(&scene, &layout);
                let events = input.take_scroll_events(&scene, &layout);
                assert!(!events.is_empty());
                for event in events {
                    event.apply(&mut ui, &scene);
                }
                if ui.scene_dirty() {
                    (scene, layout) =
                        renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
                }
            }
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            assert!(scroll_offset(&layout).is_some_and(|offset| offset > before));
            if index >= 2 {
                times.push(elapsed);
            }
        }
        times.sort_unstable_by(f64::total_cmp);
        println!(
            "long transcript ({case}, {} rows, {} warm samples) cold={initial:.2}ms p50={:.2}ms p95={:.2}ms",
            entries.len(),
            times.len(),
            times[9],
            times[18],
        );
    }
    Ok(())
}

#[test]
#[ignore = "run explicitly to measure the CPU and GPU stages of a changing transcript"]
fn transcript_gpu_benchmark() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        fmt::Write as _,
        rc::Rc,
        time::{Duration, Instant},
    };
    use xrack_layout::StackTransitions;
    use xrack_renderer_femtovg::{RenderDevice, WindowDamage};
    use xrack_runtime::Orientation;

    let follow_latest = std::env::var_os("XRACK_TRANSCRIPT_BENCH_LATEST").is_some();
    let window_entry = std::env::var_os("XRACK_TRANSCRIPT_BENCH_WINDOW_ENTRY").is_some();
    let bgra = std::env::var_os("XRACK_TRANSCRIPT_BENCH_BGRA").is_some();
    let verify_damage =
        window_entry && std::env::var_os("XRACK_TRANSCRIPT_BENCH_VERIFY_DAMAGE").is_some();
    let paragraphs = std::env::var("XRACK_TRANSCRIPT_BENCH_PARAGRAPHS")
        .map_or(Ok(0_usize), |count| count.parse())?;
    let rounds = std::env::var("XRACK_TRANSCRIPT_BENCH_ROUNDS")
        .map_or(Ok(1_usize), |rounds| rounds.parse())?;
    let passes = rounds
        .checked_mul(70)
        .filter(|&passes| passes != 0)
        .ok_or("XRACK_TRANSCRIPT_BENCH_ROUNDS is out of range")?;
    let viewport = Size::new(1120.0, 760.0);
    let scale = 2;
    let ui = transcript_benchmark_window()?;
    let entries = ui.model_entries();
    let device = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut renderer = Renderer::with_device(device.clone())?;
    if window_entry && std::env::var_os("XRACK_TRANSCRIPT_BENCH_FORCE_DAMAGE").is_some() {
        renderer.set_window_damage_forced(true);
    }
    let texture = wgpu::TextureDescriptor {
        label: Some("XRack transcript benchmark"),
        size: wgpu::Extent3d {
            width: viewport.width as u32 * scale,
            height: viewport.height as u32 * scale,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: if bgra {
            wgpu::TextureFormat::Bgra8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        },
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | if verify_damage {
                wgpu::TextureUsages::COPY_SRC
            } else {
                wgpu::TextureUsages::empty()
            },
        view_formats: &[],
    };
    let target = renderer.device.create_texture(&texture);
    let alternate = window_entry.then(|| renderer.device.create_texture(&texture));
    let mut verification = verify_damage
        .then(|| {
            let target = renderer.device.create_texture(&texture);
            Renderer::with_device(device).map(|reference| (reference, target))
        })
        .transpose()?;
    let transitions = StackTransitions::default();
    let mut input = Interaction::default();
    let (mut scene, initial_layout) =
        renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let mut layout = Rc::new(initial_layout);
    let mut long_reply = String::new();
    let mut streamed_markdown = xrack_runtime::MarkdownStream::default();
    let mut linked_markdown = file_references::TranscriptStream::default();
    for index in 0..paragraphs {
        writeln!(
            long_reply,
            "**构建 {index}** · `crates/xrack-runtime/src/lib.rs:24` · [布局](crates/xrack-layout/src/lib.rs#L4)\n"
        )?;
    }
    for streaming in [false, true] {
        let followed_viewport = if streaming && follow_latest {
            let handle = scene
                .find("transcript")
                .and_then(|node| node.viewport_handle.clone())
                .ok_or("missing transcript viewport")?;
            assert!(handle.has_bound_offset(Orientation::Vertical));
            // Layout clamps this request to the current end as the message grows.
            handle.write_bound_offset(Orientation::Vertical, f32::MAX);
            Some(handle)
        } else {
            None
        };
        let mut model = Vec::with_capacity(60);
        let mut scene_update = Vec::with_capacity(60);
        let mut layout_time = Vec::with_capacity(60);
        let mut submit = Vec::with_capacity(60);
        let mut gpu = Vec::with_capacity(60);
        let mut total = Vec::with_capacity(60);
        let mut damage_counts = [0; 3];
        let mut max_channel_delta = 0;
        for pass in 0..passes {
            let frame = pass % 70;
            input.pointer_move(&scene, &layout, Point { x: 650.0, y: 340.0 });
            let start = Instant::now();
            if streaming {
                let last = entries.len() - 1;
                let mut reply = entries.row(last).ok_or("missing streaming row")?;
                reply.body = if paragraphs == 0 {
                    streamed_markdown.update(&format!(
                        "**工作目录的分析**\n\nThe workspace build completed. 本次更新编号 {frame}。{}",
                        "\n\n渲染和交互测试需要分别执行。".repeat(frame / 6),
                    ))?
                } else {
                    writeln!(long_reply, "**输出 {pass}** · `tests/gpu_test.rs:5`\n")?;
                    file_references::transcript_document(&long_reply, Some(&mut linked_markdown))
                };
                entries.set_row(last, reply);
            }
            let updated = Instant::now();
            if followed_viewport.is_none() {
                let delta = if frame % 16 < 8 { 53.0 } else { -53.0 };
                assert!(input.scroll_wheel(&layout, Point { x: 0.0, y: delta }, false));
            }
            let base = ui.render_shared();
            let generated = Instant::now();
            let next;
            (scene, next) = renderer.prepare_scene(&ui, base, viewport, &input);
            layout = Rc::new(next);
            let prepared = Instant::now();
            let target = alternate
                .as_ref()
                .filter(|_| pass % 2 != 0)
                .unwrap_or(&target);
            let damage = if window_entry {
                Some(renderer.draw_window_prepared(
                    &scene,
                    &layout,
                    target,
                    scale as f32,
                    &input,
                    &transitions,
                )?)
            } else {
                renderer.draw_with_layout(&scene, &layout, target, scale as f32, &input)?;
                None
            };
            let submitted = Instant::now();
            renderer.device.poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            })?;
            let completed = Instant::now();
            if let Some((reference, reference_target)) = &mut verification {
                reference.draw_with_layout(
                    &scene,
                    &layout,
                    reference_target,
                    scale as f32,
                    &input,
                )?;
                let actual = gpu_readback::read_texture(&renderer, target)?;
                let expected = gpu_readback::read_texture(reference, reference_target)?;
                for (byte, (a, b)) in actual.iter().zip(&expected).enumerate() {
                    let delta = a.abs_diff(*b);
                    max_channel_delta = max_channel_delta.max(delta);
                    // The backing blit and ordinary drawing can round a
                    // channel differently at the target's 8-bit precision.
                    if delta > 1 {
                        let pixel = byte / 4;
                        let width = texture.size.width as usize;
                        let (x, y) = (pixel % width, pixel / width);
                        return Err(format!(
                            "damage {damage:?}, streaming={streaming}, frame {pass}, pixel ({x}, {y}), channel {}: {a} vs {b}",
                            byte % 4,
                        ).into());
                    }
                }
            }
            if frame >= 10 {
                if let Some(damage) = damage {
                    let index = match damage {
                        WindowDamage::Full => 0,
                        WindowDamage::Partial { .. } | WindowDamage::PartialRegions(_) => 1,
                        WindowDamage::Unchanged => 2,
                    };
                    damage_counts[index] += 1;
                }
                model.push(updated - start);
                scene_update.push(generated - updated);
                layout_time.push(prepared - generated);
                submit.push(submitted - prepared);
                gpu.push(completed - submitted);
                total.push(completed - start);
            }
        }
        if let Some(handle) = followed_viewport {
            ViewportObservers::default().sync(&scene, &layout);
            let position = handle.scroll_position(Orientation::Vertical);
            let size = handle.scroll_size(Orientation::Vertical);
            assert!(position > 0.0 && (position + size - 1.0).abs() < 1e-6);
        }
        println!(
            "transcript GPU benchmark: {} rows, streaming={streaming}, window={window_entry}, {:?}",
            entries.len(),
            texture.format,
        );
        if window_entry {
            let [full, partial, unchanged] = damage_counts;
            println!("window damage: {full} full, {partial} partial, {unchanged} unchanged");
        }
        if verify_damage {
            println!(
                "verified {passes} frames against full repaint, maximum channel delta {max_channel_delta}"
            );
        }
        for (stage, samples) in [
            ("update model", &mut model),
            ("generate scene", &mut scene_update),
            ("layout", &mut layout_time),
            ("submit", &mut submit),
            ("gpu wait", &mut gpu),
            ("total", &mut total),
        ] {
            samples.sort_unstable();
            println!(
                "{stage}: p50={:.2}ms p95={:.2}ms",
                samples[samples.len() / 2 - 1].as_secs_f64() * 1000.0,
                samples[samples.len() * 95 / 100 - 1].as_secs_f64() * 1000.0
            );
        }
    }
    Ok(())
}

#[test]
fn renders_a_workspace_with_sessions_and_a_centered_transcript()
-> Result<(), Box<dyn std::error::Error>> {
    let mut ui = AgentWorkbench::new();
    ui.set_connected(true);
    ui.set_workspace("/workspace/xrack".into());
    ui.set_workspace_name("xrack".into());
    ui.set_project_selection("/workspace/xrack".into());
    for (key, label) in [
        ("/workspace/xrack", "xrack"),
        ("/workspace/widget-lab", "widget-lab"),
        ("/workspace/desktop-tools", "desktop-tools"),
    ] {
        ui.model_project_choices().push(GenerationChoice {
            key: key.into(),
            label: label.into(),
        });
    }
    ui.set_conversation_title("梳理项目的构建入口".into());
    ui.set_conversation_id(3);
    let conversations = ui.model_conversations();
    conversations.push(ConversationRow {
        id: 3,
        title: "梳理项目的构建入口".into(),
        folder: String::new(),
        activity: String::new(),
        pinned: false,
        pinned_shown: false,
        shown: true,
    });
    conversations.push(ConversationRow {
        id: 2,
        title: "检查编辑器的预览逻辑".into(),
        folder: "工作树".into(),
        activity: "running".into(),
        pinned: false,
        pinned_shown: false,
        shown: true,
    });
    conversations.push(ConversationRow {
        id: 1,
        title: "整理控件的键盘交互".into(),
        folder: "widget-lab".into(),
        activity: String::new(),
        pinned: false,
        pinned_shown: false,
        shown: false,
    });
    ui.set_status_kind("completed".into());

    let entries = ui.model_entries();
    entries.push(TranscriptEntry {
        kind: "user".into(),
        heading: "你".into(),
        status: String::new(),
        status_tone: String::new(),
        body: StyledText::from_plain_text("把桌面计数器的运行方式补到文档里。"),
    });
    entries.push(TranscriptEntry {
        kind: "plan".into(),
        heading: "执行计划".into(),
        status: "2/2 已完成".into(),
        status_tone: String::new(),
        body: StyledText::from_plain_text("✓  定位桌面示例\n✓  核对运行命令"),
    });
    entries.push(TranscriptEntry {
        kind: "command".into(),
        heading: "ls examples/counter".into(),
        status: "已完成".into(),
        status_tone: "done".into(),
        body: StyledText::from_plain_text("BUILD.bazel\ncounter.xr\nmain.rs"),
    });
    let assistant_source = "**已补充桌面计数器的运行方式。** 构建指南和示例 README 都写明了从仓库根目录运行的命令。\n\n```sh\nbazel run //examples/counter:counter\n```";
    entries.push(TranscriptEntry {
        kind: "assistant".into(),
        heading: "Codex".into(),
        status: "已完成".into(),
        status_tone: String::new(),
        body: StyledText::from_markdown(assistant_source)?,
    });
    assert!(entries.insert(
        3,
        TranscriptEntry {
            kind: "changes".into(),
            heading: "本轮改动".into(),
            status: "2 个文件 · +4 −1".into(),
            status_tone: String::new(),
            body: StyledText::from_plain_text(""),
        }
    ));
    ui.set_renamable(true);
    ui.set_message_forks(vec![
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        "counter-docs".into(),
    ]);

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let input = Interaction::default();
    ui.set_history_more(true);
    ui.set_earlier_turns(true);
    let (scene, layout) =
        renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
    let frame = renderer.snapshot_prepared(&scene, layout.clone(), 1120, 760, 1.0, &input)?;
    if let Some(path) = std::env::var_os("XRACK_AGENT_HISTORY_SNAPSHOT") {
        frame.write_png(File::create(path)?)?;
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_SNAPSHOT") {
        frame.write_png(File::create(path)?)?;
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_TURN_ACTIVITY_SNAPSHOT") {
        let activities = ui.model_turn_activities();
        for index in 0..entries.len() {
            activities.push(TurnActivity {
                turn: (index == 1 || index == 2)
                    .then_some("turn")
                    .unwrap_or("")
                    .into(),
                summary: (index == 1)
                    .then_some("本轮活动 · 1 条命令 · 1 项计划")
                    .unwrap_or("")
                    .into(),
                ..TurnActivity::default()
            });
        }
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        let folded = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        assert_ne!(folded.rgba, frame.rgba);
        folded.write_png(File::create(path)?)?;
        activities.clear();
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_CONVERSATION_TABS_SNAPSHOT") {
        let tabs = ui.model_conversation_tabs();
        for (id, title, workspace, activity) in [
            ("1", "梳理项目的构建入口", "/workspace/xrack", ""),
            ("2", "检查编辑器预览", "/workspace/widget-lab", "running"),
            ("3", "补齐发布文档", "/workspace/desktop-tools", "updated"),
        ] {
            tabs.push(ConversationTab {
                id: id.into(),
                title: title.into(),
                workspace: workspace.into(),
                activity: activity.into(),
                archived: false,
            });
        }
        ui.set_conversation_tab("1".into());
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(scene.find("open-conversation-tabs").unwrap().is_visible());
        assert_eq!(
            scene.find("open-conversation-tabs").unwrap().children.len(),
            3
        );
        let image = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        assert_ne!(image.rgba, frame.rgba);
        image.write_png(File::create(path)?)?;
        tabs.clear();
        ui.set_conversation_tab(String::new());
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_PROMPT_EDIT_SNAPSHOT") {
        let prompt = entries.row(0).unwrap();
        assert!(entries.set_row(
            0,
            TranscriptEntry {
                status: "可编辑".into(),
                ..prompt.clone()
            }
        ));
        ui.set_draft("我正在写另一条消息。".into());
        ui.invoke_show_historical_prompt(
            "把桌面计数器的运行方式补到文档里。\n也补充 Linux 下的验证步骤。".into(),
            "counter-first".into(),
            true,
        );
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(
            scene
                .find("historical-prompt-dialog")
                .unwrap()
                .boolean("open", false)
        );
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_historical_prompt_open(false);
        ui.set_draft(String::new());
        assert!(entries.set_row(0, prompt));
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_CODE_SECTION_SNAPSHOT") {
        let mut owners = vec![String::new(); entries.len()];
        owners[4] = "|reply|".into();
        ui.set_transcript_section_owners(owners);
        let model = ui.model_transcript_sections();
        for section in
            transcript_search::fenced_sections(assistant_source, &entries.row(4).unwrap().body)
        {
            model.push(TranscriptSection {
                owner: "|reply|".into(),
                start: section.start as i64,
                language: section.language,
                annotated: section.body.clone(),
                body: section.body,
            });
        }
        assert_eq!(model.len(), 2);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        let card = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        assert_ne!(card.rgba, frame.rgba);
        card.write_png(File::create(path)?)?;
        model.clear();
        ui.set_transcript_section_owners(Vec::new());
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_HEADER_MENU_SNAPSHOT") {
        ui.set_conversation_menu_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(
            scene
                .find("conversation-menu")
                .unwrap()
                .boolean("open", false)
        );
        let menu = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        assert_ne!(menu.rgba, frame.rgba);
        menu.write_png(File::create(path)?)?;
        ui.set_conversation_menu_open(false);
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_NARROW_HEADER_SNAPSHOT") {
        ui.set_viewport_width(Length(680.0));
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(680.0, 620.0), &input);
        renderer
            .snapshot_prepared(&scene, layout, 680, 620, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_viewport_width(Length(1120.0));
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_PROJECT_GROUPS_SNAPSHOT") {
        let groups = ui.model_project_groups();
        for (key, parent, title, detail, kind, selected) in [
            (4, -1, "xrack", "运行中", "project", true),
            (7, 4, "梳理项目的构建入口", "置顶", "thread", true),
            (5, 4, "检查预览逻辑", "工作树 · 运行中", "thread", false),
            (8, -1, "widget-lab", "待确认", "project", false),
            (3, 8, "核对表格键盘操作", "待确认", "thread", false),
            (10, 8, "加载更多会话", "", "more", false),
            (12, -1, "desktop-tools", "", "project", false),
            (14, 12, "读取历史会话", "", "load", false),
        ] {
            groups.push(ProjectGroupRow {
                key,
                parent,
                conversation: (key - 1) / 2,
                title: title.into(),
                detail: detail.into(),
                kind: kind.into(),
                selected,
                ..ProjectGroupRow::default()
            });
        }
        ui.set_project_groups_open(true);
        assert!(ui.command_tree("project-group-tree", TreeCommand::Expand(4)));
        assert!(ui.command_tree("project-group-tree", TreeCommand::Expand(8)));
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        let tree = scene.find("project-group-tree").unwrap();
        assert!(tree.is_visible() && tree.children.len() == 7);
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        if let Some(path) = std::env::var_os("XRACK_AGENT_SIDEBAR_MENU_SNAPSHOT") {
            for id in 0..=3 {
                ui.model_sidebar_menus().push(SidebarMenuState {
                    pin: id > 0,
                    rename: id > 0,
                    archive: id > 0,
                    ..SidebarMenuState::default()
                });
            }
            ui.set_sidebar_menu_target(1);
            ui.set_sidebar_menu_title("核对表格键盘操作".into());
            ui.set_sidebar_menu_open(true);
            let (scene, layout) =
                renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
            assert!(
                scene
                    .find("sidebar-session-dialog")
                    .unwrap()
                    .boolean("open", false)
            );
            renderer
                .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
                .write_png(File::create(path)?)?;
            ui.set_sidebar_menu_open(false);
            ui.model_sidebar_menus().clear();
        }
        ui.set_project_groups_open(false);
        groups.clear();
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_PROJECT_SEARCH_SNAPSHOT") {
        ui.set_project_groups_open(true);
        ui.set_project_search_input("构建".into());
        ui.set_project_search_query("构建".into());
        ui.set_project_search_more(true);
        let rows = ui.model_project_search_results();
        for (id, title, folder) in [
            (3, "梳理项目的构建入口", "xrack"),
            (5, "排查桌面包构建失败", "desktop-tools · 工作树"),
            (8, "整理控件构建命令", "widget-lab"),
        ] {
            rows.push(ConversationRow {
                id,
                title: title.into(),
                folder: folder.into(),
                shown: true,
                ..ConversationRow::default()
            });
        }
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(scene.find("project-search-results").unwrap().is_visible());
        assert!(!scene.find("project-group-tree").unwrap().is_visible());
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_project_groups_open(false);
        ui.set_project_search_input(String::new());
        ui.set_project_search_query(String::new());
        ui.set_project_search_more(false);
        rows.clear();
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_TERMINAL_SNAPSHOT") {
        ui.set_terminal_open(true);
        ui.set_terminal_running(true);
        ui.set_terminal_status("本机 Shell".into());
        for id in ["1", "2"] {
            ui.model_terminal_tabs().push(TerminalTab {
                id: id.into(),
                title: format!("Shell {id}"),
            });
        }
        ui.set_terminal_tab("2".into());
        ui.set_terminal_output_available(true);
        let text = concat!(
            "xrack › pwd\n/workspace/xrack\n",
            "xrack › git status --short\n",
            " M docs/development.md\n",
            "xrack › bazel test //examples/agent-workbench:review_test\n",
            "FAIL  workbench review tests\n",
            "error: timed out while waiting for the review panel\n",
            "xrack › ▏"
        );
        let status = text.find("FAIL").unwrap();
        ui.set_terminal_screen(StyledText::from_runs(
            text,
            vec![StyledRun {
                start: status,
                end: status + 4,
                style: TextStyle {
                    color: Some(Color(0xef7c8bff)),
                    bold: true,
                    ..TextStyle::default()
                },
            }],
        )?);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(scene.find("terminal-panel").unwrap().is_visible());
        let tabs = scene.find("terminal-tabs").unwrap();
        assert_eq!(tabs.text("current-value"), Some("2"));
        assert_eq!(tabs.children.len(), 2);
        let terminal = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        assert_ne!(frame.rgba, terminal.rgba);
        terminal.write_png(File::create(path)?)?;
        if let Some(path) = std::env::var_os("XRACK_AGENT_PROJECT_ACTION_SNAPSHOT") {
            ui.set_project_actions_open(true);
            ui.set_project_action_id("workbench-check".into());
            ui.model_project_actions().push(GenerationChoice {
                key: "workbench-check".into(),
                label: "工作台测试".into(),
            });
            ui.set_project_action_command(
                "bazel test //examples/agent-workbench:review_test".into(),
            );
            ui.set_project_action_directory("/workspace/xrack".into());
            let (scene, layout) =
                renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
            assert!(
                scene
                    .find("project-actions-dialog")
                    .unwrap()
                    .boolean("open", false)
            );
            assert_eq!(
                scene
                    .find("project-action-command-preview")
                    .unwrap()
                    .text("text"),
                Some("bazel test //examples/agent-workbench:review_test")
            );
            renderer
                .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
                .write_png(File::create(path)?)?;
            ui.set_project_actions_open(false);
        }
        if let Some(path) = std::env::var_os("XRACK_AGENT_TERMINAL_QUOTE_SNAPSHOT") {
            ui.set_draft("帮我定位这次构建失败。".into());
            ui.model_attachment_rows().push(AttachmentRow {
                path: "excerpt-1".into(),
                name: "Shell 2".into(),
                kind: "终端输出".into(),
                preview: Image::Empty,
                has_preview: true,
            });
            let (scene, layout) =
                renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
            let quote = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
            assert_ne!(quote.rgba, terminal.rgba);
            quote.write_png(File::create(path)?)?;
            if let Some(path) = std::env::var_os("XRACK_AGENT_TERMINAL_PREVIEW_SNAPSHOT") {
                ui.set_attachment_preview_is_text(true);
                ui.set_attachment_preview_title("终端输出 · Shell 2".into());
                ui.set_attachment_preview_text(StyledText::from_plain_text(
                    text.trim_end_matches("▏"),
                ));
                ui.set_attachment_preview_open(true);
                let (scene, layout) = renderer.prepare_scene(
                    &ui,
                    ui.render_shared(),
                    Size::new(1120.0, 760.0),
                    &input,
                );
                assert!(scene.find("terminal-output-preview").unwrap().is_visible());
                let preview = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
                assert_ne!(preview.rgba, quote.rgba);
                preview.write_png(File::create(path)?)?;
                ui.set_attachment_preview_open(false);
                ui.set_attachment_preview_is_text(false);
            }
            ui.model_attachment_rows().clear();
            ui.set_draft(String::new());
        }
        ui.set_terminal_open(false);
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_COMMAND_GROUP_SNAPSHOT") {
        for (index, heading, status, status_tone, output) in [
            (
                3,
                "bazel test //examples/agent-workbench:workbench_parity_test",
                "失败 · 1",
                "error",
                "assertion failed: expected the send button to retain focus",
            ),
            (
                4,
                "bazel build //examples/agent-workbench:agent_workbench",
                "进行中",
                "running",
                "Compiling agent_workbench…",
            ),
        ] {
            assert!(entries.insert(
                index,
                TranscriptEntry {
                    kind: "command".into(),
                    heading: heading.into(),
                    status: status.into(),
                    status_tone: status_tone.into(),
                    body: StyledText::from_plain_text(output),
                }
            ));
        }
        let groups = ui.model_command_groups();
        let members = CommandGroup {
            first: 2,
            last: 4,
            ..CommandGroup::default()
        };
        groups.push(CommandGroup::default());
        groups.push(CommandGroup::default());
        groups.push(CommandGroup {
            status: "1 失败 · 1 进行中".into(),
            status_tone: "error".into(),
            ..members.clone()
        });
        groups.push(members.clone());
        groups.push(members);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        let collapsed = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        assert_ne!(collapsed.rgba, frame.rgba);
        collapsed.write_png(File::create(path)?)?;
        if let Some(path) = std::env::var_os("XRACK_AGENT_COMMAND_GROUP_EXPANDED_SNAPSHOT") {
            ui.set_expanded_command(3);
            let (scene, layout) =
                renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
            let expanded = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
            assert_ne!(expanded.rgba, collapsed.rgba);
            expanded.write_png(File::create(path)?)?;
            ui.set_expanded_command(-1);
        }
        entries.remove(4);
        entries.remove(3);
        groups.clear();
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_NOTIFICATIONS_SNAPSHOT") {
        ui.set_activity_open(true);
        ui.set_notifications_supported(true);
        ui.set_completion_notice_mode("background".into());
        ui.set_question_notices(true);
        ui.set_notification_settings_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(
            scene
                .find("notification-settings-dialog")
                .unwrap()
                .boolean("open", false)
        );
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_notification_settings_open(false);
        ui.set_activity_open(false);
        ui.set_notifications_supported(false);
        ui.set_completion_notice_mode("off".into());
        ui.set_question_notices(false);
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_ACTIVITY_SNAPSHOT") {
        let activity = ui.model_activity_rows();
        for (id, title, project, state) in [
            (12, "确认文件写入范围", "desktop-tools", "needs-input"),
            (11, "更新部署说明", "widget-lab", "updated"),
            (2, "检查编辑器的预览逻辑", "xrack · 工作树", "running"),
        ] {
            activity.push(ActivityRow {
                id,
                title: title.into(),
                project: project.into(),
                activity: state.into(),
            });
        }
        ui.set_activity_waiting(1);
        ui.set_activity_updated(1);
        ui.set_activity_running(1);
        ui.set_activity_can_jump(true);
        ui.set_activity_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(scene.find("activity-panel").unwrap().is_visible());
        assert_eq!(scene.find("activity-list").unwrap().children.len(), 3);
        fn layout_for<'a>(
            node: &xrack_runtime::Node,
            layout: &'a xrack_layout::LayoutNode,
            id: &str,
        ) -> Option<&'a xrack_layout::LayoutNode> {
            if node.element_id() == Some(id) {
                return Some(layout);
            }
            node.materialized_children().find_map(|(index, child)| {
                layout
                    .at_path(&[index])
                    .and_then(|subtree| layout_for(child, subtree, id))
            })
        }
        let action = layout_for(&scene, &layout, "mark-activity-read-button").unwrap();
        let activity_list = layout_for(&scene, &layout, "activity-list").unwrap();
        assert!(
            activity_list.bounds.height > 360.0,
            "activity list should fill the sidebar"
        );
        assert!(
            action.clip.contains(Point {
                x: action.bounds.x + action.bounds.width * 0.5,
                y: action.bounds.y + action.bounds.height - 1.0,
            }),
            "activity controls are clipped: {:?} within {:?}",
            action.bounds,
            action.clip
        );
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_activity_open(false);
        ui.set_activity_can_jump(false);
        ui.set_activity_waiting(0);
        ui.set_activity_updated(0);
        ui.set_activity_running(0);
        activity.clear();
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_NAVIGATION_SNAPSHOT") {
        ui.set_navigation_back(true);
        ui.set_navigation_forward(true);
        ui.set_draft("再核对一下计数器示例的构建命令。".into());
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(scene.find("navigation-back-button").unwrap().is_visible());
        assert!(
            scene
                .find("navigation-forward-button")
                .unwrap()
                .is_visible()
        );
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_navigation_back(false);
        ui.set_navigation_forward(false);
        ui.set_draft(String::new());
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_PROMPT_HISTORY_SNAPSHOT") {
        let draft = "把桌面计数器的运行方式补到文档里。\n顺便确认 Bazel 测试目标。";
        ui.set_draft(draft.into());
        ui.set_composer_recalling(true);
        ui.set_composer_recall_caret(draft.len() as i64);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(
            scene
                .find("composer-history-indicator")
                .unwrap()
                .is_visible()
        );
        let history = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        assert_ne!(history.rgba, frame.rgba);
        history.write_png(File::create(path)?)?;
        ui.set_composer_recalling(false);
        ui.set_composer_recall_caret(0);
        ui.set_draft(String::new());
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_COMPOSER_SETTINGS_SNAPSHOT") {
        ui.set_composer_enter_mode("single-line".into());
        ui.set_composer_steers(true);
        ui.set_composer_settings_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(
            scene
                .find("composer-settings-dialog")
                .unwrap()
                .boolean("open", false)
        );
        let settings = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        assert_ne!(settings.rgba, frame.rgba);
        settings.write_png(File::create(path)?)?;
        ui.set_composer_settings_open(false);
        ui.set_composer_enter_mode("modified".into());
        ui.set_composer_steers(false);
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_CHAT_FIND_SNAPSHOT") {
        let query = "桌面计数器";
        let decoration = ui.model_chat_find_documents();
        let mut found = 0;
        for row in entries.snapshot() {
            let ranges = transcript_search::matches(row.body.text(), query);
            let selected = if found == 0 { ranges.first() } else { None };
            found += ranges.len();
            decoration.push(ChatFindDocument {
                body: transcript_search::highlight(&row.body, &ranges, selected, false),
            });
        }
        assert!(found > 0);
        ui.set_chat_find_count(found as i64);
        ui.set_chat_find_position(1);
        ui.set_chat_find_query(query.into());
        ui.set_chat_find_result_query(query.into());
        ui.set_chat_find_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(scene.find("chat-find-bar").unwrap().is_visible());
        let highlighted = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        assert_ne!(highlighted.rgba, frame.rgba);
        highlighted.write_png(File::create(path)?)?;
        ui.set_chat_find_open(false);
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_PINNED_SNAPSHOT") {
        let original = conversations.row(0).unwrap();
        assert!(conversations.set_row(
            0,
            ConversationRow {
                pinned: true,
                pinned_shown: true,
                shown: false,
                ..original.clone()
            }
        ));
        ui.set_pinned_count(1);
        ui.set_conversation_pinned(true);
        ui.set_renamable(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(scene.find("pinned-session-list").unwrap().is_visible());
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        assert!(conversations.set_row(0, original));
        ui.set_pinned_count(0);
        ui.set_conversation_pinned(false);
        ui.set_renamable(false);
    }
    let mut selector = Interaction::default();
    let choice = (0..30)
        .find_map(|_| {
            selector.focus_next(&scene, &layout, false);
            selector
                .focused
                .clone()
                .filter(|target| target.element_id() == Some("project-choice"))
        })
        .expect("project selector must be keyboard focusable");
    assert!(selector.open_choice(&scene, &layout, choice));
    let projects = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &selector)?;
    assert_ne!(frame.rgba, projects.rgba);
    if let Some(path) = std::env::var_os("XRACK_AGENT_PROJECTS_SNAPSHOT") {
        projects.write_png(File::create(path)?)?;
    }
    for color in [
        [0xec, 0xf0, 0xf8, 0xff],
        [0xdd, 0xe6, 0xff, 0xff],
        [0xed, 0xf1, 0xff, 0xff],
    ] {
        assert!(
            frame.rgba.chunks_exact(4).any(|pixel| pixel == color),
            "{color:?}"
        );
    }

    ui.invoke_system_appearance_changed(ColorScheme::Dark);
    let (dark_scene, dark_layout) =
        renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
    let dark = renderer.snapshot_prepared(&dark_scene, dark_layout, 1120, 760, 1.0, &input)?;
    assert_ne!(frame.rgba, dark.rgba);
    for color in [
        [0x17, 0x1a, 0x21, 0xff],
        [0x20, 0x25, 0x2f, 0xff],
        [0x23, 0x29, 0x34, 0xff],
        [0x2b, 0x38, 0x53, 0xff],
    ] {
        assert!(dark.rgba.chunks_exact(4).any(|pixel| pixel == color));
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_DARK_SNAPSHOT") {
        dark.write_png(File::create(path)?)?;
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_APPEARANCE_SNAPSHOT") {
        ui.set_appearance_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_appearance_open(false);
    }
    ui.invoke_system_appearance_changed(ColorScheme::Light);
    if let Some(path) = std::env::var_os("XRACK_AGENT_ENGLISH_SNAPSHOT") {
        let locale = LocaleContext::default();
        locale.set_locale("en-US")?;
        ui.set_locale_context(locale);
        ui.set_appearance_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert_eq!(
            scene.find("appearance-dialog").unwrap().text("title"),
            Some("Appearance")
        );
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_appearance_open(false);
        ui.set_locale_context(LocaleContext::default());
    }

    if let Some(path) = std::env::var_os("XRACK_AGENT_FORK_SNAPSHOT") {
        ui.set_renamable(true);
        ui.set_worktree_available(true);
        for (key, label) in [
            ("", "完整会话"),
            ("later", "最近一轮 · 验证 Linux 桌面示例"),
            ("earlier", "向前 1 轮 · 梳理构建入口"),
        ] {
            ui.model_fork_choices().push(GenerationChoice {
                key: key.into(),
                label: label.into(),
            });
        }
        ui.set_fork_name("Linux 验证分支".into());
        ui.set_fork_cutoff("earlier".into());
        ui.set_fork_worktree(true);
        ui.set_fork_copy_changes(true);
        ui.set_fork_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        let fork = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        fork.write_png(File::create(path)?)?;
        assert_ne!(fork.rgba, frame.rgba);
        ui.set_fork_open(false);
        ui.set_fork_worktree(false);
        ui.set_fork_copy_changes(false);
        ui.set_renamable(false);
    }

    if let Some(path) = std::env::var_os("XRACK_AGENT_CONTEXT_SNAPSHOT") {
        ui.set_renamable(true);
        ui.set_context_known(true);
        ui.set_context_window_known(true);
        ui.set_context_fraction(19344.0 / 258400.0);
        ui.set_context_button_text("上下文 7%".into());
        ui.set_context_summary("19344 / 258400 token".into());
        ui.set_context_recent("输入 18832   ·   缓存输入 0\n输出 512   ·   推理输出 0".into());
        ui.set_context_total(
            "38688 token\n输入 37664   ·   缓存输入 0\n输出 1024   ·   推理输出 0".into(),
        );
        ui.set_context_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        let context = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        context.write_png(File::create(path)?)?;
        assert_ne!(context.rgba, frame.rgba);
        ui.set_context_open(false);
        ui.set_renamable(false);
    }

    if let Some(path) = std::env::var_os("XRACK_AGENT_ATTACHMENT_SNAPSHOT") {
        let sample =
            std::env::var_os("XRACK_AGENT_ATTACHMENT_SAMPLE").ok_or("missing PNG fixture")?;
        let bytes = std::fs::read(sample)?;
        let size = xrack_assets::image_size(ImageFormat::Png, &bytes)?;
        let image = Image::Loaded(LoadedImage::new(ImageFormat::Png, bytes, size));
        ui.model_attachment_rows().push(AttachmentRow {
            path: "/workspace/landscape.png".into(),
            name: "landscape.png".into(),
            kind: "图片".into(),
            preview: image.clone(),
            has_preview: true,
        });
        ui.model_attachment_rows().push(AttachmentRow {
            path: "/workspace/notes.md".into(),
            name: "notes.md".into(),
            kind: "文本".into(),
            preview: Image::Empty,
            has_preview: false,
        });
        ui.set_draft("参考这张图和笔记，继续完善桌面示例。".into());
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        let attachment = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        attachment.write_png(File::create(path)?)?;
        assert_ne!(attachment.rgba, frame.rgba);
        if let Some(path) = std::env::var_os("XRACK_AGENT_ATTACHMENT_PREVIEW_SNAPSHOT") {
            ui.set_attachment_preview_title("landscape.png".into());
            ui.set_attachment_preview_image(image);
            ui.set_attachment_preview_open(true);
            let (scene, layout) =
                renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
            let preview = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
            preview.write_png(File::create(path)?)?;
            assert_ne!(preview.rgba, attachment.rgba);
            ui.set_attachment_preview_open(false);
        }
        ui.set_draft(String::new());
        ui.model_attachment_rows().clear();
    }

    if let Some(path) = std::env::var_os("XRACK_AGENT_DRAFT_RESTORE_SNAPSHOT") {
        ui.set_draft("继续整理桌面示例。先用笔记核对窗口布局，再补交互测试。".into());
        ui.set_draft_storage_status("已恢复草稿".into());
        ui.model_attachment_rows().push(AttachmentRow {
            path: "/workspace/notes.md".into(),
            name: "notes.md".into(),
            kind: "文本".into(),
            preview: Image::Empty,
            has_preview: false,
        });
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(scene.find("composer-draft-status").unwrap().is_visible());
        let snapshot = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        snapshot.write_png(File::create(path)?)?;
        assert_ne!(snapshot.rgba, frame.rgba);
        ui.set_draft_storage_status(String::new());
        ui.set_draft(String::new());
        ui.model_attachment_rows().clear();
    }

    if let Some(path) = std::env::var_os("XRACK_AGENT_FOLLOWUP_SNAPSHOT") {
        ui.set_busy(true);
        ui.set_active_turn_id("turn-fixture".into());
        ui.set_queue_resumed(true);
        ui.set_status_kind(String::new());
        ui.set_status_detail("正在处理…".into());
        ui.set_draft("这轮也请检查键盘焦点。".into());
        ui.model_queued_messages().push(QueuedRow {
            id: "queue-fixture".into(),
            text: "下一轮再核对 Linux 的运行说明。".into(),
            editable: true,
        });
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        let followup = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        followup.write_png(File::create(path)?)?;
        assert_ne!(followup.rgba, frame.rgba);
        ui.set_busy(false);
        ui.set_active_turn_id(String::new());
        ui.set_queue_resumed(false);
        ui.set_status_kind("completed".into());
        ui.set_draft(String::new());
        ui.model_queued_messages().clear();
    }

    if let Some(path) = std::env::var_os("XRACK_AGENT_QUEUE_EDIT_SNAPSHOT") {
        ui.set_busy(true);
        ui.set_active_turn_id("turn-fixture".into());
        ui.set_queue_resumed(true);
        ui.set_draft("这里保留当前输入框的草稿".into());
        let queued = ui.model_queued_messages();
        for (id, text) in [
            ("first", "下一轮先核对 Linux 的运行说明"),
            ("second", "再检查链接 · $xrack-component-parity"),
        ] {
            queued.push(QueuedRow {
                id: id.into(),
                text: text.into(),
                editable: true,
            });
        }
        if let Some(path) = std::env::var_os("XRACK_AGENT_QUEUE_ORDER_SNAPSHOT") {
            let (scene, layout) =
                renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
            assert!(scene.find("move-queue-up").is_some());
            renderer
                .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
                .write_png(File::create(path)?)?;
        }
        ui.set_queue_edit_open(true);
        ui.set_queue_edit_has_attachments(true);
        ui.set_queue_edit_original("再检查链接".into());
        ui.set_queue_edit_text(
            "逐个核对链接，并补充缺失的运行命令。\n\n  bazel run //examples/counter".into(),
        );
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(
            scene
                .find("save-queued-edit-button")
                .unwrap()
                .boolean("enabled", false)
        );
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_queue_edit_open(false);
        ui.set_queue_edit_text(String::new());
        ui.set_queue_edit_original(String::new());
        ui.set_queue_edit_has_attachments(false);
        ui.set_busy(false);
        ui.set_active_turn_id(String::new());
        ui.set_queue_resumed(false);
        ui.set_draft(String::new());
        queued.clear();
    }

    if let Some(path) = std::env::var_os("XRACK_AGENT_PLAN_SNAPSHOT") {
        let baseline = entries.snapshot();
        entries.clear();
        entries.push(TranscriptEntry {
            kind: "user".into(),
            heading: "你".into(),
            status: String::new(),
            status_tone: String::new(),
            body: StyledText::from_plain_text("先规划一下 Linux 上的计数器验证。"),
        });
        entries.push(TranscriptEntry {
            kind: "proposal".into(),
            heading: "建议方案".into(),
            status: "已完成".into(),
            status_tone: String::new(),
            body: StyledText::from_markdown(
                "**验证 Linux 桌面计数器**\n\n1. 用 Bazel 构建计数器示例。\n2. 启动窗口并检查增减按钮与键盘焦点。\n3. 把启动命令和验证结果补到示例 README。"
            )?,
        });
        ui.set_conversation_title("验证 Linux 桌面示例".into());
        ui.set_earlier_turns(false);
        ui.set_plan_available(true);
        ui.set_plan_mode(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        let plan = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        plan.write_png(File::create(path)?)?;
        assert_ne!(plan.rgba, frame.rgba);
        if let Some(path) = std::env::var_os("XRACK_AGENT_QUESTION_SNAPSHOT") {
            ui.set_question_heading("验证范围 · 1 / 2".into());
            ui.set_question_text("这次需要验证哪个场景？".into());
            ui.set_question_allows_text(true);
            for (key, label) in [
                ("", "请选择"),
                ("counter", "桌面计数器"),
                ("gallery", "控件演示"),
            ] {
                ui.model_question_options().push(GenerationChoice {
                    key: key.into(),
                    label: label.into(),
                });
            }
            ui.set_question_open(true);
            ui.set_busy(true);
            let (scene, layout) =
                renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
            let question = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
            question.write_png(File::create(path)?)?;
            assert_ne!(question.rgba, plan.rgba);
            ui.set_question_open(false);
            ui.set_busy(false);
        }
        ui.set_earlier_turns(true);
        ui.set_plan_available(false);
        ui.set_plan_mode(false);
        ui.set_conversation_title("梳理项目的构建入口".into());
        entries.clear();
        for row in baseline {
            entries.push(row);
        }
    }

    if let Some(path) = std::env::var_os("XRACK_AGENT_MODEL_SNAPSHOT") {
        for (key, label) in [
            ("", "会话默认"),
            ("gpt-6-astra", "GPT-6 Astra"),
            ("gpt-6-sol", "GPT-6 Sol"),
        ] {
            ui.model_model_choices().push(GenerationChoice {
                key: key.into(),
                label: label.into(),
            });
        }
        for (key, label) in [("medium", "中"), ("high", "高"), ("xhigh", "极高")] {
            ui.model_effort_choices().push(GenerationChoice {
                key: key.into(),
                label: label.into(),
            });
        }
        ui.set_generation_model("gpt-6-astra".into());
        ui.set_generation_effort("high".into());
        ui.set_plan_available(true);
        ui.set_plan_mode(true);
        ui.set_generation_picker_open(true);
        ui.set_draft("再核对一下 Linux 的运行方式。".into());
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert_eq!(
            scene
                .find("generation-picker-trigger")
                .unwrap()
                .text("text"),
            Some("计划 · GPT-6 Astra · 高")
        );
        assert!(
            scene
                .find("generation-picker")
                .unwrap()
                .boolean("open", false)
        );
        let model_frame = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        model_frame.write_png(File::create(path)?)?;
        assert_ne!(model_frame.rgba, frame.rgba);
        ui.set_generation_picker_open(false);
        if let Some(path) = std::env::var_os("XRACK_AGENT_MODEL_NARROW_SNAPSHOT") {
            ui.set_viewport_width(Length(680.0));
            let (scene, layout) =
                renderer.prepare_scene(&ui, ui.render_shared(), Size::new(680.0, 620.0), &input);
            renderer
                .snapshot_prepared(&scene, layout, 680, 620, 1.0, &input)?
                .write_png(File::create(path)?)?;
            ui.set_viewport_width(Length(1120.0));
        }
        ui.set_plan_available(false);
        ui.set_plan_mode(false);
        ui.model_model_choices().clear();
        ui.model_effort_choices().clear();
        ui.set_generation_model(String::new());
        ui.set_generation_effort(String::new());
        ui.set_draft(String::new());
    }

    if let Some(path) = std::env::var_os("XRACK_AGENT_RENAME_SNAPSHOT") {
        ui.set_renamable(true);
        ui.set_history_query("构建".into());
        ui.set_session_search_text("构建".into());
        ui.set_history_hit_count(1);
        for (index, mut row) in conversations.snapshot().into_iter().enumerate() {
            row.shown = row.title.contains("构建");
            conversations.set_row(index, row);
        }
        ui.set_rename_draft("桌面示例构建与验证".into());
        ui.set_rename_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        let rename = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        rename.write_png(File::create(path)?)?;
        assert_ne!(rename.rgba, frame.rgba);
        ui.set_rename_open(false);
        ui.set_history_query(String::new());
        ui.set_session_search_text(String::new());
        for (index, mut row) in conversations.snapshot().into_iter().enumerate() {
            row.shown = true;
            conversations.set_row(index, row);
        }
    }

    if let Some(path) = std::env::var_os("XRACK_AGENT_ARCHIVE_SNAPSHOT") {
        ui.set_renamable(true);
        ui.set_archive_view(true);
        ui.set_conversation_archived(true);
        ui.set_history_hit_count(1);
        ui.set_status_kind("archived".into());
        ui.set_draft("继续核对桌面示例的运行方式。".into());
        let count = conversations.len();
        for index in 0..count {
            let mut row = conversations.row(index).unwrap();
            row.shown = row.id == ui.get_conversation_id();
            conversations.set_row(index, row);
        }
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        let archived = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
        archived.write_png(File::create(path)?)?;
        assert_ne!(archived.rgba, frame.rgba);
        ui.set_archive_view(false);
        ui.set_conversation_archived(false);
        ui.set_status_kind("completed".into());
        ui.set_draft(String::new());
        for (index, mut row) in conversations.snapshot().into_iter().enumerate() {
            row.shown = true;
            conversations.set_row(index, row);
        }
    }

    ui.set_review_open(true);
    ui.set_review_turn(3);
    ui.set_workspace_write(true);
    ui.set_review_status("2 个文件 · +4 −1".into());
    ui.set_review_selection("docs/development.md".into());
    let diff = concat!(
        "--- a/docs/development.md\n+++ b/docs/development.md\n",
        "@@ -3,4 +3,7 @@\n # 构建与验证\n",
        "-运行一个示例。\n\n",
        "+## 桌面计数器\n+bazel run //examples/counter:counter\n",
    );
    let removed = diff.find("-运行").unwrap();
    let added = diff.find("+##").unwrap();
    ui.set_review_body(StyledText::from_runs(
        diff,
        vec![
            StyledRun {
                start: removed,
                end: added,
                style: TextStyle {
                    color: Some(Color(0xa23b4dff)),
                    ..Default::default()
                },
            },
            StyledRun {
                start: added,
                end: diff.len(),
                style: TextStyle {
                    color: Some(Color(0x1b6b47ff)),
                    ..Default::default()
                },
            },
        ],
    )?);
    for file in [
        ReviewFile {
            path: "docs/development.md".into(),
            status: "M".into(),
        },
        ReviewFile {
            path: "examples/counter/README.md".into(),
            status: "A".into(),
        },
    ] {
        ui.model_review_files().push(file);
    }
    ui.set_expanded_command(2);
    let (scene, layout) =
        renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
    let review = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
    if let Some(path) = std::env::var_os("XRACK_AGENT_TURN_DIFF_SNAPSHOT") {
        review.write_png(File::create(path)?)?;
    }
    assert_ne!(review.rgba, frame.rgba);
    if let Some(path) = std::env::var_os("XRACK_AGENT_GIT_COMPARISON_SNAPSHOT") {
        ui.set_review_turn(-1);
        ui.set_review_comparison_mode("branch".into());
        ui.set_review_comparison_source_tree("81ac9d2e81ac9d2e81ac9d2e81ac9d2e81ac9d2e".into());
        ui.set_review_status("相对 main · 81ac9d2e · 2 个变更文件".into());
        ui.set_viewport_width(Length(740.0));
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(740.0, 560.0), &input);
        assert_eq!(
            scene.find("review-title").unwrap().text("text"),
            Some("分支对比")
        );
        assert!(!scene.find("git-change-filters").unwrap().is_visible());
        assert_eq!(
            scene.find("inspect-diff-source").unwrap().text("text"),
            Some("查看版本源码")
        );
        renderer
            .snapshot_prepared(&scene, layout, 740, 560, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_viewport_width(Length(1120.0));
        ui.set_review_turn(3);
        ui.set_review_status("2 个文件 · +4 −1".into());
        ui.set_review_comparison_mode(String::new());
        ui.set_review_comparison_source_tree(String::new());
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_STAGED_REVIEW_SNAPSHOT") {
        ui.set_review_turn(-1);
        ui.set_review_filter("staged".into());
        ui.set_review_all_count(2);
        ui.set_review_unstaged_count(2);
        ui.set_review_staged_count(1);
        ui.set_review_can_unstage(true);
        ui.set_review_status("1 个变更文件".into());
        let files = ui.model_review_files();
        let unstaged = files.remove(1).unwrap();
        ui.set_viewport_width(Length(740.0));
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(740.0, 560.0), &input);
        assert_eq!(
            scene
                .find("git-change-filters")
                .unwrap()
                .text("current-value"),
            Some("staged")
        );
        assert!(scene.find("unstage-review-file").unwrap().is_visible());
        renderer
            .snapshot_prepared(&scene, layout, 740, 560, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_viewport_width(Length(1120.0));
        files.push(unstaged);
        ui.set_review_turn(3);
        ui.set_review_filter("all".into());
        ui.set_review_status("2 个文件 · +4 −1".into());
        ui.set_review_can_unstage(false);
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_HANDOFF_SNAPSHOT") {
        let original_workspace = ui.get_workspace();
        ui.set_workspace("/worktrees/xrack/keyboard-navigation".into());
        ui.set_handoff_to_local(true);
        ui.set_handoff_source_commit("84df174a2a61".into());
        ui.set_handoff_destination("/workspace/xrack".into());
        ui.set_handoff_preview_ready(true);
        ui.set_handoff_can_copy(true);
        ui.set_handoff_copy(true);
        ui.set_handoff_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert_eq!(
            scene.find("handoff-target-label").unwrap().text("text"),
            Some("/workspace/xrack")
        );
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_handoff_open(false);
        ui.set_workspace(original_workspace);
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_WORKTREE_START_SNAPSHOT") {
        let mut draft = AgentWorkbench::new();
        draft.set_connected(true);
        draft.set_workspace("/workspace/xrack".into());
        draft.set_workspace_name("xrack".into());
        draft.set_git_checkout_label("main".into());
        draft.set_worktree_available(true);
        draft.set_draft("在隔离工作树中补齐键盘导航的回归测试".into());
        draft.set_worktree_start_choices(vec![
            "HEAD".into(),
            "feature/keyboard-nav".into(),
            "main".into(),
        ]);
        draft.set_worktree_start_base("feature/keyboard-nav".into());
        draft.set_worktree_start_commit("84df174a2a61".into());
        draft.set_worktree_start_include_count(2);
        draft.set_worktree_start_open(true);
        let (scene, layout) = renderer.prepare_scene(
            &draft,
            draft.render_shared(),
            Size::new(1120.0, 760.0),
            &input,
        );
        assert!(
            scene
                .find("use-worktree-composer")
                .unwrap()
                .boolean("enabled", false)
        );
        assert!(scene.find("worktree-include-status").unwrap().is_visible());
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_BRANCH_SNAPSHOT") {
        ui.set_review_open(false);
        ui.set_git_checkout_label("main".into());
        ui.set_git_branch_choices(
            [
                ("main", "current"),
                ("feature/keyboard-nav", ""),
                ("feature/inspector", "occupied"),
            ]
            .into_iter()
            .map(|(name, state)| workbench_ui::BranchChoice {
                name: name.into(),
                state: state.into(),
            })
            .collect(),
        );
        ui.set_git_branch_target("feature/keyboard-nav".into());
        ui.set_git_branch_ready(true);
        ui.set_git_branch_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(
            scene
                .find("confirm-git-branch-button")
                .unwrap()
                .boolean("enabled", false)
        );
        assert!(scene.find("git-branch-pill").unwrap().is_visible());
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_git_branch_open(false);
        ui.set_git_checkout_label(String::new());
        ui.set_review_open(true);
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_PR_SNAPSHOT") {
        ui.set_review_turn(-1);
        ui.set_git_pr_source(
            "github.com/contributor/xrack:feature/keyboard-nav · 858df179e613".into(),
        );
        ui.set_git_pr_repositories(vec!["team/xrack".into(), "contributor/xrack".into()]);
        ui.set_git_pr_repo("team/xrack".into());
        ui.set_git_pr_branch("main".into());
        ui.set_git_pr_title("完善计数器的键盘导航".into());
        ui.set_git_pr_body("## 改动\n\n- 保留 Tab 焦点。\n- 添加键盘操作的回归测试。".into());
        ui.set_git_pr_draft(true);
        ui.set_git_pr_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(
            scene
                .find("confirm-git-pr-button")
                .unwrap()
                .boolean("enabled", false)
        );
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_git_pr_open(false);
        ui.set_review_turn(3);
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_PUSH_SNAPSHOT") {
        ui.set_review_turn(-1);
        ui.set_git_push_source("feature/keyboard-nav".into());
        ui.set_git_push_commit("858df179e613".into());
        ui.set_git_push_remotes(vec!["origin".into(), "backup".into()]);
        ui.set_git_push_remote("origin".into());
        ui.set_git_push_address("https://git.example.invalid/workbench/xrack.git".into());
        ui.set_git_push_target("feature/keyboard-nav".into());
        ui.set_git_push_track(true);
        ui.set_git_push_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(
            scene
                .find("confirm-git-push-button")
                .unwrap()
                .boolean("enabled", false)
        );
        assert_eq!(
            scene.find("git-push-target-input").unwrap().text("text"),
            Some("feature/keyboard-nav"),
        );
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_git_push_open(false);
        ui.set_review_turn(3);
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_COMMIT_SNAPSHOT") {
        ui.set_review_turn(-1);
        ui.set_review_staged_count(2);
        ui.set_git_commit_branch("feature/keyboard-nav".into());
        ui.set_git_commit_files(vec![
            "M    examples/counter/counter.xr".into(),
            "A    docs/keyboard.md".into(),
        ]);
        ui.set_git_commit_ready(true);
        ui.set_git_commit_message("完善计数器的键盘操作\n\n补充 Tab 焦点和快捷键说明。".into());
        ui.set_git_commit_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(
            scene
                .find("confirm-git-commit-button")
                .unwrap()
                .boolean("enabled", false)
        );
        assert!(scene.find("git-commit-file-list").unwrap().is_visible());
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_git_commit_open(false);
        ui.set_review_staged_count(0);
        ui.set_review_turn(3);
    }
    ui.set_viewport_width(Length(740.0));
    let (scene, layout) =
        renderer.prepare_scene(&ui, ui.render_shared(), Size::new(740.0, 560.0), &input);
    assert!(scene.find("review-panel").unwrap().is_visible());
    assert!(!scene.find("content").unwrap().is_visible());
    assert!(!scene.find("rail").unwrap().is_visible());
    assert_eq!(
        scene.find("close-review").unwrap().text("text"),
        Some("返回对话")
    );
    if let Some(path) = std::env::var_os("XRACK_AGENT_NARROW_REVIEW_SNAPSHOT") {
        renderer
            .snapshot_prepared(&scene, layout, 740, 560, 1.0, &input)?
            .write_png(File::create(path)?)?;
    }
    ui.set_viewport_width(Length(1120.0));
    for (id, (old_line, new_line, kind, content)) in [
        (0, 0, "hunk", "@@ -3,4 +3,5 @@"),
        (3, 3, "context", "# 构建与验证"),
        (4, 0, "remove", "运行一个示例。"),
        (0, 4, "add", "## 桌面计数器"),
        (0, 5, "add", "bazel run //examples/counter:counter"),
        (5, 6, "context", "### 验证"),
        (6, 7, "context", "从仓库根目录运行。"),
    ]
    .into_iter()
    .enumerate()
    {
        ui.model_review_lines().push(ReviewLine {
            id: id as i64,
            old_line,
            new_line,
            kind: kind.into(),
            content: content.into(),
            section: String::new(),
        });
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_HUNK_REVIEW_SNAPSHOT") {
        ui.set_review_turn(-1);
        ui.set_review_filter("unstaged".into());
        ui.set_review_all_count(2);
        ui.set_review_unstaged_count(2);
        ui.set_review_can_stage(true);
        ui.set_review_can_stage_hunks(true);
        ui.set_review_status("2 个变更文件".into());
        ui.set_viewport_width(Length(740.0));
        let lines = ui.model_review_lines();
        let header = lines.row(0).unwrap();
        assert!(lines.set_row(
            0,
            ReviewLine {
                section: "未暂存".into(),
                ..header.clone()
            }
        ));
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(740.0, 560.0), &input);
        assert_eq!(
            scene.find("hunk-index-action").unwrap().text("text"),
            Some("暂存这块")
        );
        renderer
            .snapshot_prepared(&scene, layout, 740, 560, 1.0, &input)?
            .write_png(File::create(path)?)?;
        if let Some(path) = std::env::var_os("XRACK_AGENT_REVIEW_REVERT_SNAPSHOT") {
            let preview = "@@ -3,4 +3,5 @@\n # 构建与验证\n-运行一个示例。\n+## 桌面计数器\n+bazel run //examples/counter:counter\n ### 验证\n";
            let mut offset = 0;
            let runs = preview
                .split_inclusive('\n')
                .filter_map(|line| {
                    let start = offset;
                    offset += line.len();
                    let color = if line.starts_with('-') {
                        Some(Color(0xa23b4dff))
                    } else if line.starts_with('+') {
                        Some(Color(0x1b6b47ff))
                    } else {
                        None
                    }?;
                    Some(StyledRun {
                        start,
                        end: offset,
                        style: TextStyle {
                            color: Some(color),
                            ..TextStyle::default()
                        },
                    })
                })
                .collect();
            ui.set_review_revert_path("docs/development.md".into());
            ui.set_review_revert_preview(StyledText::from_runs(preview, runs)?);
            ui.set_review_revert_open(true);
            let (scene, layout) =
                renderer.prepare_scene(&ui, ui.render_shared(), Size::new(740.0, 560.0), &input);
            assert!(
                scene
                    .find("review-revert-dialog")
                    .unwrap()
                    .boolean("open", false)
            );
            renderer
                .snapshot_prepared(&scene, layout, 740, 560, 1.0, &input)?
                .write_png(File::create(path)?)?;
            ui.set_review_revert_open(false);
        }
        lines.set_row(0, header);
        ui.set_viewport_width(Length(1120.0));
        ui.set_review_turn(3);
        ui.set_review_filter("all".into());
        ui.set_review_can_stage(false);
        ui.set_review_can_stage_hunks(false);
        ui.set_review_status("2 个文件 · +4 −1".into());
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_SPLIT_REVIEW_SNAPSHOT") {
        let lines = ui.model_review_lines();
        let pairs = ui.model_review_pairs();
        let empty = ReviewLine {
            id: -1,
            ..ReviewLine::default()
        };
        for (left, right, full) in [
            (Some(0), None, true),
            (Some(1), Some(1), false),
            (Some(2), Some(3), false),
            (None, Some(4), false),
            (Some(5), Some(5), false),
            (Some(6), Some(6), false),
        ] {
            pairs.push(ReviewPair {
                id: left.or(right).unwrap() as i64,
                full,
                left: left
                    .and_then(|index| lines.row(index))
                    .unwrap_or_else(|| empty.clone()),
                right: right
                    .and_then(|index| lines.row(index))
                    .unwrap_or_else(|| empty.clone()),
            });
        }
        ui.set_review_split(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(scene.find("review-pair-list").unwrap().is_visible());
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        if let Some(path) = std::env::var_os("XRACK_AGENT_REVIEW_FIND_SNAPSHOT") {
            let matches = ui.model_review_match_rows();
            for _ in 0..lines.len() {
                matches.push(ReviewMatchRow::default());
            }
            let body = StyledText::from_plain_text(format!("+ {}", lines.row(4).unwrap().content));
            let ranges = transcript_search::matches(body.text(), "counter");
            matches.set_row(
                4,
                ReviewMatchRow {
                    marked: true,
                    current: true,
                    body: transcript_search::highlight(&body, &ranges, ranges.first(), false),
                },
            );
            ui.set_review_find_query("counter".into());
            ui.set_review_find_result_query("counter".into());
            ui.set_review_find_count(ranges.len() as i64);
            ui.set_review_find_position(1);
            ui.set_review_find_open(true);
            let (scene, layout) =
                renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
            assert!(scene.find("review-find-bar").unwrap().is_visible());
            renderer
                .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
                .write_png(File::create(path)?)?;
            ui.set_review_find_open(false);
            matches.clear();
        }
        if let Some(path) = std::env::var_os("XRACK_AGENT_REVIEW_LINE_EDIT_SNAPSHOT") {
            let turn = ui.get_review_turn();
            let counts = (ui.get_review_all_count(), ui.get_review_unstaged_count());
            ui.set_review_turn(-1);
            ui.set_review_all_count(2);
            ui.set_review_unstaged_count(2);
            for (index, mut pair) in pairs.snapshot().into_iter().enumerate() {
                if pair.right.new_line > 0 {
                    pair.right.section = "未暂存".into();
                    pairs.set_row(index, pair);
                }
            }
            ui.set_review_line_edit_location(format!("{} · 新行 5", ui.get_review_selection()));
            ui.set_review_line_edit_original("bazel run //examples/counter:counter".into());
            ui.set_review_line_edit_text(
                "bazel run //examples/counter:counter\nbazel test //examples/counter:counter_test"
                    .into(),
            );
            ui.set_review_line_edit_ready(true);
            ui.set_review_line_edit_open(true);
            let (scene, layout) =
                renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
            assert!(
                scene
                    .find("review-line-edit-dialog")
                    .unwrap()
                    .boolean("open", false)
            );
            renderer
                .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
                .write_png(File::create(path)?)?;
            ui.set_review_line_edit_open(false);
            ui.set_review_turn(turn);
            ui.set_review_all_count(counts.0);
            ui.set_review_unstaged_count(counts.1);
        }
        ui.set_review_split(false);
        pairs.clear();
    }
    ui.set_review_line_id(4);
    ui.set_review_note_reference("docs/development.md · 新行 5 · 本轮改动".into());
    ui.set_review_note_text("这里补一条 Linux 的运行说明。".into());
    let (scene, layout) =
        renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
    let note = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
    if let Some(path) = std::env::var_os("XRACK_AGENT_NOTE_SNAPSHOT") {
        note.write_png(File::create(path)?)?;
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_STAGED_REVIEW_NOTES_SNAPSHOT") {
        let reference = ui.get_review_note_reference();
        let text = ui.get_review_note_text();
        ui.set_review_note_reference(String::new());
        ui.set_review_note_text(String::new());
        let notes = ui.model_review_notes();
        for (id, reference, text) in [
            (
                1,
                "docs/development.md · 新行 5 · 未暂存",
                "补一条 Linux 上的运行说明。",
            ),
            (
                2,
                "examples/counter/main.rs · 旧行 12 · 本轮改动",
                "确认窗口关闭后计数器的状态能恢复。",
            ),
        ] {
            notes.push(ReviewNoteRow {
                id,
                reference: reference.into(),
                text: text.into(),
            });
        }
        ui.set_review_notes_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(
            scene
                .find("staged-review-notes-dialog")
                .unwrap()
                .boolean("open", false)
        );
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_review_notes_open(false);
        notes.clear();
        ui.set_review_note_reference(reference);
        ui.set_review_note_text(text);
    }
    assert_ne!(review.rgba, note.rgba);

    ui.invoke_select_appearance(ColorScheme::Dark);
    let (scene, layout) =
        renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
    let dark_review = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
    assert_ne!(note.rgba, dark_review.rgba);
    for color in [[0x20, 0x3b, 0x35, 0xff], [0x40, 0x2e, 0x38, 0xff]] {
        assert!(dark_review.rgba.chunks_exact(4).any(|pixel| pixel == color));
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_DARK_REVIEW_SNAPSHOT") {
        dark_review.write_png(File::create(path)?)?;
    }
    ui.invoke_select_appearance(ColorScheme::System);
    ui.set_review_open(false);
    ui.set_review_turn(-1);
    ui.set_workspace_write(false);
    ui.set_expanded_command(-1);

    let label = "examples/counter/main.rs:12-13";
    let report = StyledText::from_markdown(&format!(
        "计数器恢复会话时仍显示初始值。\n\n**[P2] 恢复时同步计数器标签**\n\n{label}\n\n组件恢复后应使用持久化的计数值更新标签。",
    ))?;
    let start = report.text().find(label).unwrap();
    ui.set_review_report_document(StyledText::from_parts(
        report.text(),
        report.runs().to_vec(),
        vec![StyledLink {
            start,
            end: start + label.len(),
            href: "review-source:12:13:examples/counter/main.rs".into(),
        }],
    )?);
    ui.set_review_report_open(true);
    let (scene, layout) =
        renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
    let report = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
    if let Some(path) = std::env::var_os("XRACK_AGENT_CODE_REVIEW_SNAPSHOT") {
        report.write_png(File::create(path)?)?;
    }
    assert_ne!(frame.rgba, report.rgba);
    ui.set_review_report_open(false);

    ui.set_review_open(true);
    ui.set_files_active(true);
    ui.set_workspace_preview_tab("counter".into());
    for (id, title, path) in [
        ("readme", "README.md", "README.md"),
        ("counter", "counter.xr", "examples/counter/counter.xr"),
        ("main", "main.rs", "examples/counter/main.rs"),
    ] {
        ui.model_workspace_preview_tabs().push(WorkspacePreviewTab {
            id: id.into(),
            title: title.into(),
            path: path.into(),
            dirty: false,
        });
    }
    ui.set_workspace_file_key(5);
    ui.set_workspace_file_path("examples/counter/counter.xr".into());
    ui.set_workspace_file_kind("text".into());
    ui.set_workspace_file_text(StyledText::from_plain_text(concat!(
        "   1  export component Counter inherits Window {\n",
        "   2      title: \"Counter\";\n",
        "   3      in-out property <int> count: 0;\n",
        "   4\n",
        "   5      Button {\n",
        "   6          text: \"Count \" + root.count;\n",
        "   7          clicked => { root.count += 1; }\n",
        "   8      }\n",
        "   9  }\n",
    )));
    for (id, parent_id, path, name, kind) in [
        (1, -1, "crates", "crates", "directory"),
        (2, 1, "crates/xrack", "xrack", "directory"),
        (3, -1, "examples", "examples", "directory"),
        (4, 3, "examples/counter", "counter", "directory"),
        (5, 4, "examples/counter/counter.xr", "counter.xr", "file"),
        (6, 4, "examples/counter/main.rs", "main.rs", "file"),
        (7, -1, "README.md", "README.md", "file"),
    ] {
        ui.model_workspace_files().push(WorkspaceFile {
            id,
            parent_id,
            path: path.into(),
            name: name.into(),
            kind: kind.into(),
        });
    }
    assert!(ui.command_tree("workspace-tree", TreeCommand::ExpandTo(5)));
    let (scene, layout) =
        renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
    let files = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
    if let Some(path) = std::env::var_os("XRACK_AGENT_FILES_SNAPSHOT") {
        files.write_png(File::create(path)?)?;
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_FILE_TABS_SNAPSHOT") {
        files.write_png(File::create(path)?)?;
    }
    if let Some(path) = std::env::var_os("XRACK_AGENT_EXTERNAL_OPEN_SNAPSHOT") {
        ui.set_external_open_path("examples/counter/counter.xr:7:11".into());
        ui.set_external_destination("vscode".into());
        ui.set_external_can_remember(true);
        ui.set_external_remember(true);
        ui.set_external_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert_eq!(
            scene.find("external-open-location").unwrap().text("text"),
            Some("examples/counter/counter.xr:7:11"),
        );
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_external_open(false);
    }
    assert_ne!(frame.rgba, files.rgba);
    if let Some(path) = std::env::var_os("XRACK_AGENT_REFERENCE_SNAPSHOT") {
        entries.clear();
        ui.set_earlier_turns(false);
        entries.push(TranscriptEntry {
            kind: "user".into(),
            heading: "你".into(),
            status: String::new(),
            status_tone: String::new(),
            body: StyledText::from_plain_text("计数器里的点击处理在哪？"),
        });
        entries.push(TranscriptEntry {
            kind: "assistant".into(),
            heading: "Codex".into(),
            status: String::new(),
            status_tone: String::new(),
            body: StyledText::from_markdown(
                "点击事件在 [counter.xr:7](workspace-file:examples/counter/counter.xr:7) 更新计数。"
            )?,
        });
        let source = concat!(
            "      5      Button {\n",
            "      6          text: \"Count \" + root.count;\n",
            "›     7          clicked => { root.count += 1; }\n",
            "      8      }\n",
            "      9  }\n",
        );
        let start = source.find('›').unwrap();
        let end = start + source[start..].find('\n').unwrap();
        ui.set_workspace_file_positioned(true);
        ui.set_workspace_file_status("已定位第 7 行".into());
        ui.set_workspace_file_text(StyledText::from_runs(
            source,
            vec![StyledRun {
                start,
                end,
                style: TextStyle {
                    bold: true,
                    color: Some(Color(0x445bc5ff)),
                    ..TextStyle::default()
                },
            }],
        )?);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
    }
    let context_snapshot = std::env::var_os("XRACK_AGENT_FILE_CONTEXT_SNAPSHOT");
    if let Some(path) = context_snapshot
        .clone()
        .or_else(|| std::env::var_os("XRACK_AGENT_FILE_EDITOR_SNAPSHOT"))
    {
        let tabs = ui.model_workspace_preview_tabs();
        let mut current = tabs.row(1).unwrap();
        current.dirty = true;
        assert!(tabs.set_row(1, current));
        ui.set_workspace_file_editing(true);
        ui.set_workspace_file_edit_dirty(true);
        ui.set_workspace_edits_dirty(true);
        ui.set_workspace_file_edit_status("未保存".into());
        ui.set_workspace_file_edit_text(
            concat!(
                "export component Counter inherits Window {\n",
                "    title: \"Counter\";\n",
                "    in-out property <int> count: 0;\n\n",
                "    Button {\n",
                "        text: \"Count \" + root.count;\n",
                "        clicked => { root.count += 2; }\n",
                "    }\n",
                "}\n",
            )
            .into(),
        );
        if context_snapshot.is_some() {
            ui.model_attachment_rows().push(AttachmentRow {
                path: "excerpt-0".into(),
                name: "counter.xr:7".into(),
                kind: "草稿选段".into(),
                preview: Image::Empty,
                has_preview: false,
            });
            ui.set_draft("帮我看看这个步长改成 2 后，测试该怎么调整。".into());
        }
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_workspace_file_editing(false);
        ui.set_workspace_edits_dirty(false);
        if context_snapshot.is_some() {
            ui.model_attachment_rows().clear();
            ui.set_draft(String::new());
        }
    }
    ui.set_review_open(false);
    ui.set_files_active(false);

    if let Some(path) = std::env::var_os("XRACK_AGENT_SLASH_SNAPSHOT") {
        ui.set_draft("/".into());
        let source = ui.render_shared();
        let handle = source.find("input").unwrap().focus_handle.as_ref().unwrap();
        handle.publish(true, true);
        handle.publish_text_selection(1, 1..1, "");
        ui.set_slash_command_draft("/".into());
        ui.set_slash_command_caret(1);
        for (title, detail) in [
            ("/model", "选择模型和推理档位"),
            ("/plan", "开启计划模式"),
            ("/review", "审阅代码"),
            ("/fork", "从当前会话创建分支"),
            ("/diff", "查看工作区改动"),
            ("/terminal", "打开终端"),
        ] {
            ui.model_slash_command_items().push(PaletteItem {
                key: title.into(),
                title: title.into(),
                detail: detail.into(),
                enabled: true,
            });
        }
        ui.set_slash_command_index(0);
        ui.set_slash_commands_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        assert!(scene.find("slash-command-results").unwrap().is_visible());
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_slash_commands_open(false);
        handle.publish(false, false);
        ui.set_draft(String::new());
        ui.model_slash_command_items().clear();
    }

    for (env, skill) in [
        ("XRACK_AGENT_FILE_MENTION_SNAPSHOT", false),
        ("XRACK_AGENT_SKILL_MENTION_SNAPSHOT", true),
    ] {
        let Some(path) = std::env::var_os(env) else {
            continue;
        };
        let draft = if skill {
            "检查桌面界面，用 $xrack"
        } else {
            "请检查 @counter"
        };
        let caret = draft.len();
        ui.set_draft(draft.into());
        let source = ui.render_shared();
        let handle = source.find("input").unwrap().focus_handle.as_ref().unwrap();
        handle.publish(true, true);
        handle.publish_text_selection(caret, caret..caret, "");
        ui.set_file_mention_draft(draft.into());
        ui.set_file_mention_caret(caret as i64);
        ui.set_file_mention_open(true);
        ui.set_skill_mention(skill);
        ui.set_skill_mention_state("ready".into());
        ui.model_file_mention_items().clear();
        let candidates = if skill {
            [
                ("$xrack-component-parity", "对照 Rust AOT 和 MLIR 解释器"),
                ("$xrack-language-lsp", "同步更新 .xr 语言和编辑器支持"),
                ("$xrack-rendering", "验证 GPU 绘制和输入交互"),
            ]
        } else {
            [
                ("counter.xr", "examples/counter"),
                ("counter.xr", "examples/cargo-consumer/ui"),
                ("main.rs", "examples/counter"),
            ]
        };
        for (title, detail) in candidates {
            ui.model_file_mention_items().push(PaletteItem {
                key: format!("{detail}/{title}"),
                title: title.into(),
                detail: detail.into(),
                enabled: true,
            });
        }
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        let candidates = scene.find("file-mention-list").expect("file mention list");
        assert!(candidates.is_visible());
        assert_eq!(
            candidates.text("accessible-name"),
            Some(if skill {
                "可添加到草稿的技能"
            } else {
                "可附加的工作区文件"
            })
        );
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.invoke_dismiss_file_mention();
        handle.publish(false, false);
        ui.set_draft(String::new());
    }

    if let Some(path) = std::env::var_os("XRACK_AGENT_PALETTE_SNAPSHOT") {
        ui.set_palette_query("counter:7".into());
        ui.set_palette_result_query("counter:7".into());
        ui.set_palette_file_mode(true);
        ui.set_palette_ready(true);
        ui.set_palette_index(0);
        for (key, title, detail) in [
            (
                "examples/counter/counter.xr",
                "counter.xr",
                "examples/counter",
            ),
            (
                "examples/cargo-consumer/ui/counter.xr",
                "counter.xr",
                "examples/cargo-consumer/ui",
            ),
            ("examples/counter/main.rs", "main.rs", "examples/counter"),
            (
                "examples/counter/BUILD.bazel",
                "BUILD.bazel",
                "examples/counter",
            ),
        ] {
            ui.model_palette_items().push(PaletteItem {
                key: key.into(),
                title: title.into(),
                detail: detail.into(),
                enabled: true,
            });
        }
        ui.set_palette_open(true);
        let (scene, layout) =
            renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?
            .write_png(File::create(path)?)?;
        ui.set_palette_open(false);
    }

    if let Some(path) = std::env::var_os("XRACK_AGENT_LATEST_MESSAGE_SNAPSHOT") {
        for (kind, heading, body) in [
            ("user", "你", "请把桌面计数器做成仓库的验证入口。"),
            (
                "assistant",
                "Codex",
                "我会先核对三个入口。\n\n构建目标负责生成可执行文件。\n测试目标覆盖点击和状态更新。\nREADME 记录运行方式。",
            ),
            ("user", "你", "Linux 和 macOS 的运行方式能统一吗？"),
            (
                "assistant",
                "Codex",
                "两边都通过 Bazel 启动同一个 Rust 目标。\n\n从仓库根目录运行：\n\nbazel run //examples/counter:counter\n\n验证时可以直接执行示例的测试目标。",
            ),
            ("user", "你", "把测试命令也加进去。"),
            (
                "assistant",
                "Codex",
                "已在示例说明里加上运行和测试命令。\n\n下一步可以核对图形适配器初始化和键盘焦点。",
            ),
        ] {
            entries.push(TranscriptEntry {
                kind: kind.into(),
                heading: heading.into(),
                status: String::new(),
                status_tone: String::new(),
                body: StyledText::from_plain_text(body),
            });
        }
        let mut scrolling = Interaction::default();
        let size = Size::new(1120.0, 760.0);
        let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &scrolling);
        scrolling.pointer_move(&scene, &layout, Point { x: 675.0, y: 340.0 });
        assert!(scrolling.scroll_wheel(&layout, Point { x: 0.0, y: 300.0 }, false));
        let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &scrolling);
        ViewportObservers::default().sync(&scene, &layout);
        let events = scrolling.take_scroll_events(&scene, &layout);
        assert_eq!(events.len(), 1);
        for event in events {
            assert!(event.apply(&mut ui, &scene));
        }
        let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &scrolling);
        assert!(scene.find("latest-message").unwrap().is_visible());
        renderer
            .snapshot_prepared(&scene, layout, 1120, 760, 1.0, &scrolling)?
            .write_png(File::create(path)?)?;
    }

    entries.clear();
    ui.set_earlier_turns(false);
    assert!(conversations.insert(
        0,
        ConversationRow {
            id: 4,
            title: "新会话".into(),
            folder: "xrack".into(),
            activity: String::new(),
            pinned: false,
            pinned_shown: false,
            shown: true,
        }
    ));
    ui.set_conversation_id(4);
    ui.set_conversation_title("新会话".into());
    ui.set_status_kind("ready-read".into());
    let (scene, layout) =
        renderer.prepare_scene(&ui, ui.render_shared(), Size::new(1120.0, 760.0), &input);
    let welcome = renderer.snapshot_prepared(&scene, layout, 1120, 760, 1.0, &input)?;
    if let Some(path) = std::env::var_os("XRACK_AGENT_WELCOME_SNAPSHOT") {
        welcome.write_png(File::create(path)?)?;
    }
    assert_ne!(frame.rgba, welcome.rgba);
    Ok(())
}
