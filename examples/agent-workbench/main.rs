mod activity;
mod appearance;
mod attachments;
mod chat_find;
mod command_groups;
mod composer_drafts;
mod conversation_tabs;
mod environment;
mod external_open;
mod file_references;
mod handoff;
mod mention;
mod notifications;
mod palette;
mod project_sidebar;
mod prompt_history;
mod queued_editor;
mod review;
mod review_reports;
mod settings;
mod sidebar_actions;
mod terminal;
mod turn_activity;
mod workspace_files;
mod worktree_include;
mod worktrees;

use clap::Parser;
use session::{
    ActivityState, Client, Commands, ConversationActivity, Entry, Input, Snapshot, Status,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    env,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    thread,
};
use tokio::sync::{oneshot, watch};
use workbench_ui::{
    AgentWorkbench, ConversationRow, GenerationChoice, QueuedRow, TranscriptEntry,
    TranscriptSection,
};
use xrack_platform_winit::{run_with_viewport_and_system_color_scheme, spawn_local};
use xrack_runtime::{Length, StyledText};

#[derive(Parser)]
struct Args {
    /// Path to the Codex CLI.
    #[arg(long, default_value = "codex")]
    codex: PathBuf,
    /// Open the workbench in this directory.
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Allow Codex to edit files using its workspace-write sandbox.
    #[arg(long)]
    workspace_write: bool,
}

fn workspace_path(path: &Path) -> Result<String, String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("无法打开工作目录：{error}"))?;
    if !path.is_dir() {
        return Err("请选择目录".into());
    }
    path.into_os_string()
        .into_string()
        .map_err(|_| "工作目录必须是 UTF-8 路径".into())
}

fn folder_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_owned()
}

fn activity_status(state: ActivityState) -> &'static str {
    match state {
        ActivityState::Pending => "待处理",
        ActivityState::Running => "进行中",
        ActivityState::Completed => "已完成",
        ActivityState::Failed => "失败",
        ActivityState::Declined => "已拒绝",
        ActivityState::Interrupted => "已停止",
        ActivityState::Unfinished => "未完成",
    }
}

fn transcript_row(
    entry: &Entry,
    markdown: Option<&mut file_references::TranscriptStream>,
) -> TranscriptEntry {
    let blank = String::new();
    let (kind, heading, status, tone, body) = match entry {
        Entry::User(message) => (
            "user",
            "你".into(),
            if message.prompt_text().trim().is_empty() {
                blank
            } else {
                "可编辑".into()
            },
            "",
            StyledText::from_plain_text(&message.display),
        ),
        Entry::Assistant { text, finalized } => (
            "assistant",
            "Codex".into(),
            if *finalized && !text.trim().is_empty() {
                "已完成".into()
            } else {
                blank
            },
            "",
            file_references::transcript_document(text, markdown),
        ),
        Entry::Proposal { text, finalized } => (
            "proposal",
            "建议方案".into(),
            if *finalized { "已完成" } else { "草稿" }.into(),
            "",
            file_references::transcript_document(text, markdown),
        ),
        Entry::Command(activity) => {
            let state = activity.state;
            let tone = match state {
                ActivityState::Running => "running",
                ActivityState::Completed => "done",
                ActivityState::Failed | ActivityState::Declined => "error",
                _ => "",
            };
            let mut output = if activity.output.is_empty() {
                if state == ActivityState::Running {
                    "等待命令输出…".into()
                } else {
                    "没有命令输出".into()
                }
            } else {
                activity.output.clone()
            };
            if activity.output_truncated {
                output.push_str("\n\n输出已截断。");
            }
            let status = activity_status(state);
            (
                "command",
                activity.command.clone(),
                match activity.exit_code.filter(|code| *code != 0) {
                    Some(code) => format!("{status} · {code}"),
                    None => status.into(),
                },
                tone,
                StyledText::from_plain_text(output),
            )
        }
        Entry::Plan(plan) => {
            let mut summary = String::new();
            if let Some(text) = &plan.explanation
                && !text.is_empty()
            {
                summary.push_str(text);
                summary.push_str("\n\n");
            }
            let done = plan
                .steps
                .iter()
                .filter(|step| step.state == ActivityState::Completed)
                .count();
            for step in &plan.steps {
                if !summary.is_empty() && !summary.ends_with('\n') {
                    summary.push('\n');
                }
                summary.push_str(match step.state {
                    ActivityState::Completed => "✓  ",
                    ActivityState::Running => "↳  ",
                    _ => "○  ",
                });
                summary.push_str(&step.text);
                if matches!(
                    step.state,
                    ActivityState::Running | ActivityState::Interrupted | ActivityState::Unfinished
                ) {
                    summary.push_str(" · ");
                    summary.push_str(activity_status(step.state));
                }
            }
            (
                "plan",
                "执行计划".into(),
                format!("{done}/{} 已完成", plan.steps.len()),
                "",
                StyledText::from_plain_text(summary),
            )
        }
        Entry::Changes(diff) => (
            "changes",
            "本轮改动".into(),
            review::turn_summary(diff),
            "",
            StyledText::from_plain_text(""),
        ),
        Entry::Compaction => (
            "compaction",
            "上下文已压缩".into(),
            blank,
            "",
            StyledText::from_plain_text(""),
        ),
        Entry::ReviewReport(report) => {
            let (summary, locations) = review_reports::report_summary(report);
            (
                "code-review",
                "代码审阅".into(),
                if locations == 0 {
                    "已完成".into()
                } else {
                    format!("{locations} 处可定位意见")
                },
                "",
                StyledText::from_markdown(summary)
                    .unwrap_or_else(|_| StyledText::from_plain_text(summary)),
            )
        }
    };
    TranscriptEntry {
        kind: kind.into(),
        heading,
        status,
        status_tone: tone.into(),
        body,
    }
}

fn update_conversations(ui: &mut AgentWorkbench, snapshot: &Snapshot, previous: Option<&Snapshot>) {
    ui.set_navigation_back(snapshot.navigation_back);
    ui.set_navigation_forward(snapshot.navigation_forward);
    let model = ui.model_conversations();
    let in_pin_group = |state: &Snapshot, row: &session::ConversationSummary| {
        state.shows_conversation(row)
            && row.pinned
            && !state.archive_view
            && state.history_query.trim().is_empty()
    };
    // New conversations are prepended. History pages append, preserving existing row order.
    let mut old = previous
        .map_or(&[][..], |prior| prior.conversations.as_slice())
        .iter()
        .peekable();
    let mut shown_count = 0;
    let mut pinned_count = 0;
    for (index, entry) in snapshot.conversations.iter().enumerate() {
        let shown = snapshot.shows_conversation(entry);
        let pinned_shown = in_pin_group(snapshot, entry);
        shown_count += i64::from(shown);
        pinned_count += i64::from(pinned_shown);
        let existing = old.next_if(|prior| prior.id == entry.id);
        if existing == Some(entry)
            && previous.is_some_and(|prior| {
                prior.shows_conversation(entry) == shown
                    && in_pin_group(prior, entry) == pinned_shown
            })
        {
            continue;
        }
        let row = ConversationRow {
            id: entry.id,
            title: entry.title.clone(),
            folder: entry
                .project_workspace
                .as_ref()
                .map_or_else(String::new, |_| "工作树".into()),
            activity: entry.activity.key().into(),
            shown: shown && !pinned_shown,
            pinned: entry.pinned && !entry.archived,
            pinned_shown,
        };
        if existing.is_some() {
            model.set_row(index, row);
        } else {
            model.insert(index, row);
        }
    }
    ui.set_conversation_id(snapshot.view.conversation);
    ui.set_pinned_count(pinned_count);
    ui.set_pin_status(snapshot.pin_status.clone());
    ui.set_conversation_pinned(
        snapshot
            .conversations
            .iter()
            .any(|row| row.id == snapshot.view.conversation && row.pinned),
    );
    if let Some(entry) = snapshot
        .conversations
        .iter()
        .find(|row| row.id == snapshot.view.conversation)
    {
        ui.set_conversation_title(entry.title.clone());
    }
    let name = snapshot
        .conversations
        .iter()
        .find(|row| row.id == snapshot.view.conversation)
        .and_then(|row| row.project_workspace.as_deref())
        .map_or_else(
            || folder_name(&snapshot.view.workspace),
            |project| format!("{} · 工作树", folder_name(project)),
        );
    ui.set_workspace_name(name);
    ui.set_switching(snapshot.view.restoring);
    ui.set_history_loading(snapshot.history_loading);
    ui.set_history_more(snapshot.history_more);
    ui.set_history_status(snapshot.history_status.clone());
    ui.set_history_query(snapshot.history_query.clone());
    ui.set_history_hit_count(shown_count);
    ui.set_renamable(snapshot.view.renamable);
    if ui.get_rename_pending()
        && !snapshot.view.rename_pending
        && snapshot.view.rename_error.is_empty()
    {
        ui.set_rename_open(false);
    }
    ui.set_rename_pending(snapshot.view.rename_pending);
    ui.set_rename_error(snapshot.view.rename_error.clone());
    ui.set_archive_view(snapshot.archive_view);
    ui.set_archive_pending(snapshot.view.archive_pending);
    ui.set_conversation_archived(snapshot.current_archived());
    ui.set_earlier_turns(snapshot.view.earlier_turns);
}

fn update_projects(ui: &mut AgentWorkbench, snapshot: &Snapshot, previous: Option<&Snapshot>) {
    let projects_changed =
        previous.is_none_or(|prior| !Arc::ptr_eq(&prior.projects, &snapshot.projects));
    if projects_changed
        || previous.is_some_and(|prior| prior.conversations != snapshot.conversations)
    {
        let names: Vec<_> = snapshot
            .projects
            .iter()
            .map(|path| folder_name(path))
            .collect();
        let model = ui.model_project_choices();
        if projects_changed {
            model.clear();
        }
        for (index, (path, name)) in snapshot.projects.iter().zip(&names).enumerate() {
            let mut label = if names.iter().filter(|other| *other == name).count() > 1 {
                let parent = Path::new(path)
                    .parent()
                    .and_then(Path::to_str)
                    .unwrap_or(path);
                format!("{name} · {parent}")
            } else {
                name.clone()
            };
            let mut running = 0;
            let mut attention = false;
            let mut updated = false;
            for row in snapshot
                .conversations
                .iter()
                .filter(|row| row.project() == path)
            {
                match row.activity {
                    ConversationActivity::Running => running += 1,
                    ConversationActivity::NeedsInput => attention = true,
                    ConversationActivity::Updated => updated = true,
                    ConversationActivity::Idle => {}
                }
            }
            if attention {
                label.push_str(" · 待确认");
            } else if running > 0 {
                label.push_str(&format!(" · {running} 项运行中"));
            } else if updated {
                label.push_str(" · 有更新");
            }
            let choice = GenerationChoice {
                key: path.clone(),
                label,
            };
            if projects_changed {
                model.push(choice);
            } else if model
                .row(index)
                .is_some_and(|row| row.label != choice.label)
            {
                model.set_row(index, choice);
            }
        }
    }
    ui.set_project_selection(snapshot.project_workspace().into());
    ui.set_project_status(snapshot.project_status.clone());
}

fn is_streaming_text(entry: &Entry) -> bool {
    matches!(
        entry,
        Entry::Assistant {
            finalized: false,
            ..
        } | Entry::Proposal {
            finalized: false,
            ..
        }
    )
}

fn update(
    ui: &mut AgentWorkbench,
    snapshot: &Snapshot,
    previous: Option<&Snapshot>,
    markdown: &mut file_references::TranscriptStream,
) {
    sidebar_actions::update(ui, snapshot, previous);
    update_projects(ui, snapshot, previous);
    update_conversations(ui, snapshot, previous);
    activity::publish(ui, snapshot, previous);
    update_generation(ui, snapshot, previous);
    update_question(ui, snapshot, previous);
    update_followups(ui, snapshot, previous);
    ui.set_handoff_pending(snapshot.view.handoff_pending);
    if previous.is_none_or(|old| {
        old.view.handoff_error != snapshot.view.handoff_error
            || old.view.handoff_pending && !snapshot.view.handoff_pending
    }) && !snapshot.view.handoff_error.is_empty()
    {
        ui.set_handoff_error(snapshot.view.handoff_error.clone());
    }
    update_context(ui, snapshot, previous);
    update_fork(ui, snapshot, previous);
    let model = ui.model_entries();
    let conversation_changed =
        previous.is_some_and(|old| old.view.conversation != snapshot.view.conversation);
    let activity_changed = previous.is_none_or(|old| {
        conversation_changed || !Arc::ptr_eq(&old.view.fork_points, &snapshot.view.fork_points)
    });
    if previous.is_some_and(|old| old.project_workspace() != snapshot.project_workspace()) {
        ui.set_session_search_text(snapshot.history_query.clone());
    }
    if conversation_changed {
        *markdown = file_references::TranscriptStream::default();
        ui.set_handoff_open(false);
        ui.set_worktree_start_open(false);
        ui.set_worktree_setup_log_open(false);
        ui.set_worktree_start_error(String::new());
        ui.set_git_branch_open(false);
        ui.set_git_pr_open(false);
        ui.set_git_push_open(false);
        ui.set_git_commit_open(false);
        ui.set_external_open(false);
        ui.set_review_revert_open(false);
        ui.set_review_notes_open(false);
        ui.set_review_line_edit_open(false);
        ui.invoke_close_review_find(false);
        ui.invoke_dismiss_file_mention();
        ui.set_slash_commands_open(false);
        ui.invoke_end_composer_recall();
        ui.invoke_close_chat_find();
        ui.set_chat_find_query(String::new());
        ui.model_chat_find_documents().clear();
        ui.invoke_reveal_latest();
        ui.set_rename_open(false);
        ui.set_conversation_menu_open(false);
        ui.set_generation_picker_open(false);
        ui.set_context_open(false);
        ui.set_fork_open(false);
        ui.set_historical_prompt_open(false);
        ui.set_message_quote_open(false);
        ui.set_message_quote_body(StyledText::default());
        ui.set_message_copy_index(-1);
        ui.set_review_scope_open(false);
        ui.set_review_report_open(false);
        ui.set_review_source_open(false);
    }
    mention::sync_skills(ui, snapshot, previous);
    let previous = if let Some(previous) =
        previous.filter(|old| old.view.conversation == snapshot.view.conversation)
    {
        previous.view.entries.as_slice()
    } else {
        model.clear();
        ui.model_transcript_sections().clear();
        ui.set_transcript_section_owners(Vec::new());
        ui.model_command_groups().clear();
        ui.set_expanded_command(-1);
        ui.set_expanded_command_group(-1);
        &[]
    };
    // Older pages prepend immutable entries. Keep the rows for the existing
    // messages, along with their expansion and review selection.
    let added = snapshot.view.entries.len().saturating_sub(previous.len());
    let prepended = previous
        .first()
        .zip(snapshot.view.entries.get(added))
        .filter(|(first, new)| Arc::ptr_eq(first, new))
        .map_or(0, |_| added);
    if prepended > 0 {
        ui.invoke_preserve_transcript_anchor();
        let offset = prepended as i64;
        let copied = ui.get_message_copy_index();
        if copied >= 0 {
            ui.set_message_copy_index(copied + offset);
        }
        let expanded = ui.get_expanded_command();
        if expanded >= 0 {
            ui.set_expanded_command(expanded + offset);
        }
        let group = ui.get_expanded_command_group();
        if group >= 0 {
            ui.set_expanded_command_group(group + offset);
        }
        let reviewed = ui.get_review_turn();
        if reviewed >= 0 {
            ui.set_review_turn(reviewed + offset);
        }
    }
    let mut changed = false;
    let mut grouping_changed = previous.len() != snapshot.view.entries.len();
    let review_turn = usize::try_from(ui.get_review_turn()).ok();
    let mut review_updated = false;
    let mut streaming = false;
    let mut section_updates = Vec::new();
    for (index, entry) in snapshot.view.entries.iter().enumerate() {
        let streaming_entry = is_streaming_text(entry);
        streaming |= streaming_entry;
        let before = index
            .checked_sub(prepended)
            .and_then(|index| previous.get(index));
        if before.is_some_and(|old| Arc::ptr_eq(old, entry)) {
            continue;
        }
        changed = true;
        grouping_changed |=
            before.and_then(|entry| command_groups::state(entry)) != command_groups::state(entry);
        review_updated |= review_turn == Some(index);
        let markdown = (streaming_entry || before.is_some_and(|old| is_streaming_text(old)))
            .then_some(&mut *markdown);
        let row = transcript_row(entry, markdown);
        let sections = match entry.as_ref() {
            Entry::Assistant {
                text,
                finalized: true,
            } => transcript_search::fenced_sections(text, &row.body),
            _ => Vec::new(),
        };
        if !sections.is_empty()
            || before.is_some_and(|entry| {
                matches!(
                    entry.as_ref(),
                    Entry::Assistant {
                        finalized: true,
                        ..
                    }
                )
            })
        {
            section_updates.push((index, sections));
        }
        if index < prepended {
            model.insert(index, row);
        } else if before.is_some() {
            model.set_row(index, row);
        } else {
            model.push(row);
        }
    }
    if !streaming {
        *markdown = file_references::TranscriptStream::default();
    }
    let removed = model.len() > snapshot.view.entries.len();
    while model.len() > snapshot.view.entries.len() {
        model.remove(model.len() - 1);
        changed = true;
    }
    if prepended > 0 || removed || !section_updates.is_empty() {
        update_transcript_sections(ui, prepended, section_updates);
    }
    let activity_rows_changed =
        activity_changed && turn_activity::sync(ui, snapshot, !conversation_changed);
    if grouping_changed || activity_rows_changed {
        command_groups::sync(ui, &snapshot.view.entries);
    }
    changed |= activity_rows_changed;
    let workspace_changed = ui.get_workspace() != snapshot.view.workspace;
    if workspace_changed {
        ui.set_handoff_open(false);
        ui.set_worktree_start_open(false);
        ui.set_worktree_setup_log_open(false);
        ui.set_worktree_start_error(String::new());
        ui.set_git_branch_open(false);
        ui.set_git_checkout_label(String::new());
        ui.set_git_pr_open(false);
        ui.set_git_push_open(false);
        ui.set_git_commit_open(false);
        ui.set_external_open(false);
        ui.set_review_revert_open(false);
        ui.set_review_notes_open(false);
        ui.set_review_line_edit_open(false);
        ui.invoke_dismiss_file_mention();
        ui.set_historical_prompt_open(false);
        ui.set_slash_commands_open(false);
        ui.set_conversation_menu_open(false);
        ui.set_generation_picker_open(false);
        ui.set_palette_open(false);
        ui.set_session_search_text(snapshot.history_query.clone());
        ui.set_workspace(snapshot.view.workspace.clone());
        ui.invoke_refresh_git_checkout();
        ui.invoke_reset_workspace_files();
        ui.model_terminal_tabs().clear();
        ui.set_terminal_tab(String::new());
        ui.set_project_actions_open(false);
        if ui.get_terminal_open() {
            ui.invoke_sync_terminal(false);
        }
    }
    if workspace_changed || conversation_changed && review_turn.is_some() {
        review::workspace_changed(ui);
    } else if review_updated && ui.get_review_open() && !ui.get_files_active() {
        ui.invoke_refresh_review();
    }
    ui.set_connected(snapshot.connected);
    ui.set_busy(snapshot.view.busy);
    ui.set_reviewing(snapshot.view.reviewing);
    ui.set_stopping(snapshot.view.stopping);
    if !snapshot.view.busy && !ui.get_fork_open() && !snapshot.fork_error.is_empty() {
        show_status(ui, snapshot.fork_error.as_str());
    } else {
        publish_status(ui, &snapshot.view.status);
    }
    if changed {
        if prepended == 0 {
            ui.invoke_transcript_updated();
        }
        if ui.get_chat_find_open() {
            ui.invoke_refresh_chat_find();
        }
    }
}

fn publish_status(ui: &mut AgentWorkbench, status: &Status) {
    let (kind, detail) = status.presentation();
    if ui.get_status_kind() != kind {
        ui.set_status_kind(kind.into());
    }
    if ui.get_status_detail() != detail {
        ui.set_status_detail(detail.into());
    }
}

fn show_status(ui: &mut AgentWorkbench, status: impl Into<Status>) {
    publish_status(ui, &status.into());
}

fn update_transcript_sections(
    ui: &mut AgentWorkbench,
    prepended: usize,
    updates: Vec<(usize, Vec<transcript_search::Section>)>,
) {
    let mut owners = ui.get_transcript_section_owners();
    if owners.is_empty() && updates.iter().all(|(_, sections)| sections.is_empty()) {
        return;
    }
    let rows = ui.model_entries();
    let model = ui.model_transcript_sections();
    let mut replaced = HashSet::new();
    let mut inserted = Vec::new();
    if prepended > 0 && !owners.is_empty() {
        owners.splice(..0, std::iter::repeat_n(String::new(), prepended));
    }
    if owners.len() > rows.len() {
        replaced.extend(owners.drain(rows.len()..));
    }
    owners.resize(rows.len(), String::new());
    for (index, sections) in updates {
        let Some(key) = rows.row_key(index) else {
            continue;
        };
        let old_owner = std::mem::take(&mut owners[index]);
        if !old_owner.is_empty() {
            replaced.insert(old_owner);
        }
        if sections.is_empty() {
            continue;
        }
        let owner = format!("|{key:?}|");
        inserted.extend(sections.into_iter().map(|section| TranscriptSection {
            owner: owner.clone(),
            start: section.start as i64,
            language: section.language,
            annotated: section.body.clone(),
            body: section.body,
        }));
        owners[index] = owner;
    }
    if !replaced.is_empty() {
        model.retain(|section| !replaced.contains(&section.owner));
    }
    for section in inserted {
        model.push(section);
    }
    if model.is_empty() {
        owners.clear();
    }
    ui.set_transcript_section_owners(owners);
}

fn update_fork(ui: &mut AgentWorkbench, snapshot: &Snapshot, previous: Option<&Snapshot>) {
    ui.set_fork_pending(snapshot.fork_pending);
    ui.set_fork_error(snapshot.fork_error.clone());
    if previous.is_none_or(|old| !Arc::ptr_eq(&old.view.fork_points, &snapshot.view.fork_points)) {
        let mut message_forks = vec![String::new(); snapshot.view.entries.len()];
        let choices = ui.model_fork_choices();
        choices.clear();
        choices.push(GenerationChoice {
            key: String::new(),
            label: "完整会话".into(),
        });
        for (index, point) in snapshot.view.fork_points.iter().rev().enumerate() {
            if let Some(cutoff) = point.response.and_then(|row| message_forks.get_mut(row)) {
                cutoff.clone_from(&point.id);
            }
            let position = if index == 0 {
                "最近一轮".into()
            } else {
                format!("向前 {index} 轮")
            };
            choices.push(GenerationChoice {
                key: point.id.clone(),
                label: format!("{position} · {}", point.label),
            });
        }
        ui.set_message_forks(message_forks);
    }
    if previous.is_some_and(|old| old.forks_created != snapshot.forks_created) {
        ui.set_fork_open(false);
        ui.set_sessions_open(true);
        ui.set_session_search_text(snapshot.history_query.clone());
    }
}

fn update_context(ui: &mut AgentWorkbench, snapshot: &Snapshot, previous: Option<&Snapshot>) {
    ui.set_compacting(snapshot.view.compacting);
    if previous.is_some_and(|old| old.view.context_usage == snapshot.view.context_usage) {
        return;
    }
    let Some(usage) = &snapshot.view.context_usage else {
        ui.set_context_known(false);
        ui.set_context_window_known(false);
        ui.set_context_fraction(0.0);
        ui.set_context_button_text("用量".into());
        ui.set_context_summary("发起一轮对话后显示用量。".into());
        ui.set_context_recent(String::new());
        ui.set_context_total(String::new());
        return;
    };
    let fraction = usage.fraction();
    ui.set_context_known(true);
    ui.set_context_window_known(fraction.is_some());
    ui.set_context_fraction(fraction.unwrap_or_default());
    ui.set_context_button_text(fraction.map_or_else(
        || "用量".into(),
        |fraction| format!("上下文 {:.0}%", fraction * 100.0),
    ));
    ui.set_context_summary(match usage.window {
        Some(window) => format!("{} / {window} token", usage.last.total),
        None => format!("最近一次测量 {} token", usage.last.total),
    });
    let details = |counts: &session::TokenCounts| {
        format!(
            "输入 {}   ·   缓存输入 {}\n输出 {}   ·   推理输出 {}",
            counts.input, counts.cached, counts.output, counts.reasoning,
        )
    };
    ui.set_context_recent(details(&usage.last));
    ui.set_context_total(format!(
        "{} token\n{}",
        usage.total.total,
        details(&usage.total)
    ));
}

fn update_followups(ui: &mut AgentWorkbench, snapshot: &Snapshot, previous: Option<&Snapshot>) {
    let send_resolved = ui.get_sending_prompt() && !snapshot.view.prompt_pending;
    let followup_resolved = ui.get_followup_pending() && !snapshot.view.followup_pending;
    if send_resolved || followup_resolved {
        let accepted = if send_resolved {
            snapshot.view.sent_prompts != previous.map_or(0, |old| old.view.sent_prompts)
        } else {
            snapshot.view.followup_error.is_empty()
        };
        if accepted {
            let submitted = ui.get_submitted_draft();
            if ui.get_draft() == submitted {
                ui.set_draft(String::new());
            }
            ui.invoke_remember_composer_prompt(submitted);
            ui.invoke_clear_attachments();
        }
        ui.set_submitted_draft(String::new());
        ui.set_sending_prompt(false);
    }
    ui.set_followup_pending(snapshot.view.followup_pending);
    ui.set_queue_loading(snapshot.view.queue_loading);
    ui.set_queue_pending(snapshot.view.queue_pending);
    ui.set_queue_resumed(snapshot.view.queue_resumed);
    ui.set_active_turn_id(snapshot.view.active_turn_id.clone());
    if previous.is_none_or(|old| !Arc::ptr_eq(&old.view.queued, &snapshot.view.queued)) {
        let rows = ui.model_queued_messages();
        for (index, item) in snapshot.view.queued.iter().enumerate() {
            let row = QueuedRow {
                id: item.id.clone(),
                text: item.text.lines().collect::<Vec<_>>().join(" · "),
                editable: item.editable_text().is_some(),
            };
            if rows.row(index).is_none_or(|existing| existing.id != row.id) {
                if let Some(old) = (index + 1..rows.len())
                    .find(|old| rows.row(*old).is_some_and(|existing| existing.id == row.id))
                {
                    rows.move_row(old, index);
                } else {
                    rows.insert(index, row.clone());
                }
            }
            if rows.row(index).is_some_and(|existing| existing != row) {
                rows.set_row(index, row);
            }
        }
        while rows.len() > snapshot.view.queued.len() {
            rows.remove(rows.len() - 1);
        }
    }
}

fn update_generation(ui: &mut AgentWorkbench, snapshot: &Snapshot, previous: Option<&Snapshot>) {
    let catalog_changed = previous.is_none_or(|old| !Arc::ptr_eq(&old.models, &snapshot.models));
    let selected = snapshot.view.turn_options.supported(&snapshot.models);
    if catalog_changed {
        let choices = ui.model_model_choices();
        choices.clear();
        choices.push(GenerationChoice {
            key: String::new(),
            label: "会话默认".into(),
        });
        for model in snapshot.models.iter() {
            choices.push(GenerationChoice {
                key: model.id.clone(),
                label: model.name.clone(),
            });
        }
    }
    if catalog_changed
        || previous
            .is_none_or(|old| old.view.turn_options.model != snapshot.view.turn_options.model)
    {
        let choices = ui.model_effort_choices();
        choices.clear();
        if let Some(model) = snapshot
            .models
            .iter()
            .find(|model| model.id == selected.model)
        {
            if model.default_effort.is_empty() {
                choices.push(GenerationChoice {
                    key: String::new(),
                    label: "默认".into(),
                });
            }
            for effort in &model.efforts {
                let label = match effort.as_str() {
                    "none" => "关闭",
                    "minimal" => "最少",
                    "low" => "低",
                    "medium" => "中",
                    "high" => "高",
                    "xhigh" => "极高",
                    "max" => "最大",
                    "ultra" => "超高",
                    unknown => unknown,
                };
                choices.push(GenerationChoice {
                    key: effort.clone(),
                    label: label.into(),
                });
            }
        } else {
            choices.push(GenerationChoice {
                key: String::new(),
                label: "默认".into(),
            });
        }
    }
    ui.set_generation_model(selected.model);
    ui.set_generation_effort(selected.effort);
    ui.set_models_loading(snapshot.models_loading);
    ui.set_models_status(snapshot.models_status.clone());
    ui.set_plan_available(snapshot.plan_available);
    ui.set_plan_mode(snapshot.view.plan_mode);
    if snapshot.current_archived()
        || (snapshot.models.is_empty()
            && !snapshot.models_loading
            && snapshot.models_status.is_empty()
            && !snapshot.plan_available)
    {
        ui.set_generation_picker_open(false);
    }
}

fn update_question(ui: &mut AgentWorkbench, snapshot: &Snapshot, previous: Option<&Snapshot>) {
    if previous.is_some_and(|old| old.view.question == snapshot.view.question) {
        return;
    }
    ui.set_question_choice(String::new());
    ui.set_question_draft(String::new());
    ui.set_question_deferred(false);
    let choices = ui.model_question_options();
    choices.clear();
    if let Some(page) = &snapshot.view.question {
        ui.invoke_dismiss_file_mention();
        ui.set_palette_open(false);
        ui.set_context_open(false);
        ui.set_generation_picker_open(false);
        ui.set_question_token(page.token);
        ui.set_question_index(page.index as i64);
        ui.set_question_heading(if page.header.is_empty() {
            format!("第 {} / {} 题", page.index + 1, page.total)
        } else {
            format!("{}  ·  {} / {}", page.header, page.index + 1, page.total)
        });
        ui.set_question_text(page.text.clone());
        ui.set_question_allows_text(page.allows_text);
        ui.set_question_secret(page.secret);
        if !page.options.is_empty() {
            choices.push(GenerationChoice {
                key: String::new(),
                label: "请选择".into(),
            });
            for option in &page.options {
                let label = if option.description.is_empty() {
                    option.label.clone()
                } else {
                    format!("{} · {}", option.label, option.description)
                };
                choices.push(GenerationChoice {
                    key: option.label.clone(),
                    label,
                });
            }
        }
    }
    ui.set_question_open(snapshot.view.question.is_some());
}

fn dispatch(ui: &mut AgentWorkbench, sender: &Commands, input: Input) -> bool {
    match sender.send(input) {
        Ok(()) => true,
        Err(error) => {
            show_status(ui, error);
            false
        }
    }
}

async fn read_in_background<T: Send + 'static>(
    worker: &str,
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (sender, receiver) = oneshot::channel();
    thread::Builder::new()
        .name(worker.into())
        .spawn(move || {
            let _ = sender.send(job());
        })
        .map_err(|error| error.to_string())?;
    receiver
        .await
        .map_err(|_| "后台文件操作已结束".to_owned())?
}

fn can_navigate(ui: &AgentWorkbench) -> bool {
    !ui.get_switching()
        && !ui.get_handoff_preparing()
        && !ui.get_handoff_pending()
        && !ui.get_git_branch_pending()
        && !ui.get_review_line_edit_open()
        && !ui.get_sending_prompt()
        && !ui.get_followup_pending()
        && !ui.get_queue_pending()
        && !ui.get_fork_pending()
        && !ui.get_worktree_preparing()
        && !ui.get_archive_pending()
        && !ui.get_rename_pending()
}

#[derive(Default)]
struct ConversationUiState {
    draft: String,
    note: review::NoteDraft,
    review_notes: Vec<session::SavedReviewNote>,
    files: Vec<attachments::DraftFile>,
    reading: Option<Length>,
    expanded_commands: Option<(i64, i64)>,
    expanded_turns: HashSet<String>,
}

impl ConversationUiState {
    fn capture(ui: &AgentWorkbench, attachments: &attachments::Attachments) -> Self {
        Self {
            draft: ui.get_draft(),
            note: review::NoteDraft::capture(ui),
            review_notes: review::capture_notes(ui),
            files: attachments.capture(),
            reading: (!ui.get_follow_output()).then(|| ui.get_transcript_offset()),
            expanded_commands: Some((ui.get_expanded_command(), ui.get_expanded_command_group())),
            expanded_turns: turn_activity::expanded(ui),
        }
    }
}

fn attach(
    ui: &mut AgentWorkbench,
    client: &Client,
    output: watch::Receiver<Snapshot>,
    worktree_base: Option<PathBuf>,
    notices: notifications::Notifications,
    draft_store: session::DraftStore,
) {
    chat_find::attach(ui);
    turn_activity::attach(ui);
    prompt_history::attach(ui, output.clone());
    let attachments = attachments::attach(ui);
    terminal::attach(ui, &attachments);
    let drafts = composer_drafts::ComposerDrafts::attach(ui, &attachments, draft_store);
    mention::attach(ui, &attachments, client.commands(), output.clone());
    let queue_editor = queued_editor::QueuedEditor::attach(ui, client.commands(), output.clone());
    let composer_worktrees = worktrees::ComposerWorktrees::attach(ui, worktree_base.clone());
    handoff::attach(ui, output.clone(), client.commands(), worktree_base.clone());
    palette::attach(ui);
    workspace_files::attach(ui, &attachments);
    let sidebar = project_sidebar::attach(ui, output.clone(), client.commands());
    sidebar_actions::attach(ui, client, output.clone());
    let conversation_tabs = conversation_tabs::attach(ui, Some(client.commands()));
    let session_attachments = attachments.clone();
    let plans = output.clone();
    let project_output = output.clone();
    let activity_output = output.clone();
    let rendered_snapshot = Rc::new(RefCell::new(None::<Rc<Snapshot>>));
    let prompt_snapshot = rendered_snapshot.clone();
    let weak = ui.as_weak();
    ui.on_edit_historical_prompt(move |index| {
        let Some(mut ui) = weak
            .upgrade()
            .filter(|ui| !ui.get_switching() && !ui.get_conversation_archived())
        else {
            return;
        };
        let snapshot = prompt_snapshot.borrow().clone();
        if let Some((text, turn, attachments)) = snapshot
            .as_deref()
            .and_then(|snapshot| historical_prompt(snapshot, ui.get_conversation_id(), index))
        {
            ui.invoke_show_historical_prompt(text.into(), turn.into(), attachments);
        }
    });
    let copy_snapshot = rendered_snapshot.clone();
    let weak = ui.as_weak();
    let clipboard = Rc::new(RefCell::new(None));
    let message_clipboard = clipboard.clone();
    ui.on_copy_message(move |index| {
        let Some(mut ui) = weak.upgrade().filter(|ui| !ui.get_switching()) else {
            return;
        };
        let snapshot = copy_snapshot.borrow().clone();
        let Some(text) = snapshot
            .as_deref()
            .and_then(|snapshot| copyable_response(snapshot, ui.get_conversation_id(), index))
        else {
            return;
        };
        copy_transcript_text(
            &mut ui,
            &mut message_clipboard.borrow_mut(),
            index,
            -1,
            text,
        );
    });
    let weak = ui.as_weak();
    let copy_snapshot = rendered_snapshot.clone();
    ui.on_copy_code_section(move |index, offset, code| {
        let Some(mut ui) = weak.upgrade().filter(|ui| !ui.get_switching()) else {
            return;
        };
        if copy_snapshot.borrow().as_deref().is_some_and(|snapshot| {
            copyable_response(snapshot, ui.get_conversation_id(), index).is_some()
        }) {
            copy_transcript_text(
                &mut ui,
                &mut clipboard.borrow_mut(),
                index,
                offset,
                code.text(),
            );
        }
    });
    let weak = ui.as_weak();
    ui.on_quote_message(move |selected| {
        if let Some(mut ui) = weak.upgrade()
            && !ui.get_switching()
            && !ui.get_conversation_archived()
            && append_message_quote(&mut ui, &selected)
        {
            ui.set_message_quote_open(false);
            ui.set_message_quote_body(StyledText::default());
            ui.invoke_focus_composer();
        }
    });
    let weak = ui.as_weak();
    let commands = client.commands();
    let worktree_settings = composer_worktrees.clone();
    ui.on_attach(move || {
        notifications::start();
        if let Some(mut ui) = weak.upgrade() {
            ui.invoke_refresh_git_checkout();
        }
        let notices = notices.clone();
        let weak = weak.clone();
        let task_weak = weak.clone();
        let commands = commands.clone();
        let mut output = output.clone();
        let attachments = session_attachments.clone();
        let sidebar = sidebar.clone();
        let conversation_tabs = conversation_tabs.clone();
        let queue_editor = queue_editor.clone();
        let worktree_settings = worktree_settings.clone();
        let drafts = drafts.clone();
        let rendered_snapshot = rendered_snapshot.clone();
        if let Err(error) = spawn_local(async move {
            let mut saved = HashMap::new();
            let mut answers = HashMap::new();
            let mut markdown = file_references::TranscriptStream::default();
            loop {
                let snapshot = output.borrow_and_update().clone();
                if commands.is_current(&snapshot) {
                    let Some(mut ui) = task_weak.upgrade() else {
                        return;
                    };
                    let previous = rendered_snapshot.borrow().clone();
                    let mut reading = None;
                    let mut expanded = None;
                    let mut expanded_turns = None;
                    let mut focus_seeded_draft = false;
                    let mut restore = previous.as_ref().is_none_or(|prior| !prior.composer_ready);
                    if let Some(prior) = &previous
                        && prior.view.conversation != snapshot.view.conversation
                    {
                        drafts.borrow_mut().leave(&mut ui, &attachments);
                        if !prior.view.workspace.is_empty() {
                            saved.insert(
                                prior.view.conversation,
                                ConversationUiState::capture(&ui, &attachments),
                            );
                        }
                        let restored = saved.remove(&snapshot.view.conversation);
                        restore = restored.is_none();
                        focus_seeded_draft = restore && snapshot.view.initial_draft.is_some();
                        let restored = restored.unwrap_or_else(|| ConversationUiState {
                            draft: snapshot
                                .view
                                .initial_draft
                                .as_deref()
                                .unwrap_or_default()
                                .to_owned(),
                            ..ConversationUiState::default()
                        });
                        reading = restored.reading;
                        expanded = restored.expanded_commands;
                        expanded_turns = Some(restored.expanded_turns);
                        ui.set_draft(restored.draft);
                        review::restore_notes(&ui, restored.review_notes);
                        restored.note.restore(&mut ui);
                        attachments.restore(&mut ui, restored.files);
                        if let Some(question) = &prior.view.question {
                            let choice = ui.get_question_choice();
                            let text = ui.get_question_draft();
                            if !choice.is_empty() || !text.is_empty() {
                                answers.insert(
                                    prior.view.conversation,
                                    (question.token, question.index, choice, text),
                                );
                            }
                        }
                    }
                    let awaiting_send = ui.get_sending_prompt() || ui.get_followup_pending();
                    update(&mut ui, &snapshot, previous.as_deref(), &mut markdown);
                    worktree_settings.sync(&mut ui);
                    queue_editor.sync(&mut ui, &snapshot, previous.as_deref());
                    if let Some(open) = expanded_turns {
                        turn_activity::restore(&ui, &open);
                    }
                    conversation_tabs.borrow_mut().sync(&mut ui, &snapshot);
                    sidebar
                        .borrow_mut()
                        .update(&mut ui, &snapshot, previous.as_deref());
                    let mut drafts = drafts.borrow_mut();
                    drafts.show(&mut ui, &attachments, &snapshot, restore);
                    if focus_seeded_draft {
                        ui.invoke_focus_composer();
                        ui.invoke_restore_composer_caret(ui.get_draft().len() as i64);
                    }
                    if awaiting_send && !ui.get_sending_prompt() && !ui.get_followup_pending() {
                        drafts.save(&mut ui, &attachments);
                        ui.invoke_draft_checkpoint();
                    }
                    if let Some((command, group)) = expanded {
                        ui.set_expanded_command(command);
                        ui.set_expanded_command_group(group);
                    }
                    if let Some(offset) = reading {
                        ui.invoke_restore_reading_position(offset);
                    }
                    if let Some((token, index, choice, text)) =
                        answers.remove(&snapshot.view.conversation)
                        && snapshot
                            .view
                            .question
                            .as_ref()
                            .is_some_and(|page| page.token == token && page.index == index)
                    {
                        ui.set_question_choice(choice);
                        ui.set_question_draft(text);
                    }
                    notices.update(&mut ui, &snapshot, previous.as_deref());
                    rendered_snapshot.replace(Some(Rc::new(snapshot)));
                }
                if output.changed().await.is_err() {
                    return;
                }
            }
        }) && let Some(mut ui) = weak.upgrade()
        {
            show_status(&mut ui, error.to_string());
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_load_session_history(move |more| {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_connected()
            && !ui.get_history_loading()
            && dispatch(&mut ui, &commands, Input::History { more })
        {
            ui.set_history_loading(true);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_search_sessions(move |query| {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_connected()
            && !ui.get_workspace().is_empty()
            && dispatch(&mut ui, &commands, Input::Search(query))
        {
            ui.set_history_loading(true);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_view_archived_sessions(move |archived| {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_connected()
            && !ui.get_workspace().is_empty()
            && dispatch(&mut ui, &commands, Input::ArchiveView(archived))
        {
            ui.set_history_loading(true);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_set_session_archived(move |archived| {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_connected()
            && ui.get_renamable()
            && !ui.get_busy()
            && !ui.get_switching()
            && !ui.get_archive_pending()
            && !ui.get_rename_pending()
            && dispatch(&mut ui, &commands, Input::SetArchived(archived))
        {
            ui.set_archive_pending(true);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_set_session_pinned(move |pinned| {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_connected()
            && ui.get_renamable()
            && !ui.get_switching()
            && !ui.get_conversation_archived()
            && !ui.get_archive_pending()
        {
            let conversation = ui.get_conversation_id();
            dispatch(
                &mut ui,
                &commands,
                Input::SetPinned {
                    conversation,
                    pinned,
                },
            );
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_rename_conversation(move |name| {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_renamable()
            && ui.get_connected()
            && !ui.get_busy()
            && !ui.get_switching()
            && !ui.get_rename_pending()
            && !ui.get_archive_pending()
            && dispatch(&mut ui, &commands, Input::Rename(name))
        {
            ui.set_rename_pending(true);
            ui.set_rename_error(String::new());
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_load_earlier_turns(move || {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_connected()
            && ui.get_earlier_turns()
            && !ui.get_busy()
            && !ui.get_switching()
            && dispatch(&mut ui, &commands, Input::EarlierTurns)
        {
            ui.set_switching(true);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_select_workspace(move |path| {
        let Some(mut ui) = weak.upgrade() else { return };
        let active = project_output.borrow().project_workspace().to_owned();
        if can_navigate(&ui) {
            match workspace_path(Path::new(&path)) {
                Ok(path) if path != active => {
                    if dispatch(&mut ui, &commands, Input::Workspace(path)) {
                        ui.set_switching(true);
                        return;
                    }
                }
                Ok(_) => {}
                Err(error) => show_status(&mut ui, error),
            }
        }
        ui.set_project_selection(active);
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_new_conversation(move || {
        if let Some(mut ui) = weak.upgrade()
            && can_navigate(&ui)
            && dispatch(&mut ui, &commands, Input::Reset)
        {
            ui.set_session_search_text(String::new());
            ui.set_switching(true);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_select_conversation(move |id| {
        if let Some(mut ui) = weak.upgrade()
            && can_navigate(&ui)
            && dispatch(&mut ui, &commands, Input::Select(id))
        {
            ui.set_switching(true);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_navigate_conversation(move |forward| {
        if let Some(mut ui) = weak.upgrade()
            && can_navigate(&ui)
            && (if forward {
                ui.get_navigation_forward()
            } else {
                ui.get_navigation_back()
            })
            && dispatch(&mut ui, &commands, Input::Navigate { forward })
        {
            ui.set_switching(true);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_mark_activity_read(move || {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_activity_updated() > 0
        {
            dispatch(&mut ui, &commands, Input::MarkActivityRead);
        }
    });

    let weak = ui.as_weak();
    ui.on_open_next_attention(move || {
        if let Some(mut ui) = weak.upgrade()
            && can_navigate(&ui)
            && let Some(id) = activity::next_attention(&activity_output.borrow())
        {
            ui.invoke_select_conversation(id);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_load_models(move || {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_connected()
            && !ui.get_models_loading()
            && dispatch(&mut ui, &commands, Input::LoadModels)
        {
            ui.set_models_loading(true);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_choose_model(move |model| {
        if let Some(mut ui) = weak.upgrade() {
            dispatch(&mut ui, &commands, Input::ChooseModel(model));
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_choose_effort(move |effort| {
        if let Some(mut ui) = weak.upgrade() {
            dispatch(&mut ui, &commands, Input::ChooseEffort(effort));
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_choose_plan_mode(move |plan| {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_plan_available()
            && !ui.get_busy()
            && !ui.get_switching()
            && dispatch(&mut ui, &commands, Input::ChoosePlanMode(plan))
        {
            ui.set_plan_mode(plan);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_answer_question(move |token, index, answer, skip| {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_question_open()
            && !ui.get_stopping()
        {
            dispatch(
                &mut ui,
                &commands,
                Input::AnswerQuestion {
                    token,
                    index,
                    value: (!skip).then_some(answer),
                },
            );
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_use_plan(move |index| {
        let Some(mut ui) = weak.upgrade() else { return };
        if ui.get_busy()
            || ui.get_switching()
            || ui.get_conversation_archived()
            || ui.get_followup_pending()
        {
            return;
        }
        let snapshot = plans.borrow();
        let Some(Entry::Proposal {
            text,
            finalized: true,
        }) = usize::try_from(index)
            .ok()
            .filter(|_| snapshot.view.conversation == ui.get_conversation_id())
            .and_then(|index| snapshot.view.entries.get(index))
            .map(AsRef::as_ref)
        else {
            return;
        };
        if ui.get_plan_available() && !dispatch(&mut ui, &commands, Input::ChoosePlanMode(false)) {
            return;
        }
        ui.set_plan_mode(false);
        let mut draft = ui.get_draft();
        if !draft.is_empty() {
            draft.push_str("\n\n");
        }
        draft.push_str("按以下方案继续执行。\n\n");
        draft.push_str(text);
        ui.set_draft(draft);
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_create_fork(move |title, last_turn, use_worktree, copy_changes| {
        if let Some(mut ui) = weak.upgrade().filter(can_request_fork) {
            if use_worktree {
                if let Some(base) = &worktree_base {
                    worktrees::prepare_fork(
                        &mut ui,
                        base.clone(),
                        commands.clone(),
                        worktrees::ForkPreparation {
                            title,
                            last_turn,
                            copy_changes,
                        },
                    );
                }
            } else if dispatch(
                &mut ui,
                &commands,
                Input::Fork {
                    title,
                    last_turn,
                    worktree: None,
                },
            ) {
                ui.set_fork_pending(true);
            }
        }
    });
    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_fork_historical_prompt(move |turn, draft| {
        let Some(mut ui) = weak.upgrade().filter(can_request_fork) else {
            return;
        };
        if !ui.get_historical_prompt_open()
            || turn.is_empty()
            || turn != ui.get_historical_prompt_turn()
            || draft.trim().is_empty()
        {
            return;
        }
        let conversation = ui.get_conversation_id();
        ui.set_historical_prompt_error(String::new());
        if dispatch(
            &mut ui,
            &commands,
            Input::ForkBeforePrompt {
                conversation,
                turn,
                draft,
            },
        ) {
            ui.set_historical_prompt_fork_requested(true);
            ui.set_fork_error(String::new());
            ui.set_fork_pending(true);
        } else {
            ui.set_historical_prompt_error(ui.get_status());
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_compact_context(move || {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_connected()
            && ui.get_renamable()
            && !ui.get_busy()
            && !ui.get_switching()
            && !ui.get_queue_pending()
            && !ui.get_queue_loading()
            && !ui.get_followup_pending()
            && !ui.get_conversation_archived()
            && !ui.get_fork_pending()
            && ui.model_queued_messages().is_empty()
            && dispatch(&mut ui, &commands, Input::Compact)
        {
            ui.set_context_open(false);
            ui.set_compacting(true);
            ui.set_busy(true);
            show_status(&mut ui, Status::Compacting);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    let followup_attachments = attachments.clone();
    ui.on_followup(move |text, steer| {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_busy()
            && !ui.get_switching()
            && !ui.get_stopping()
            && !ui.get_compacting()
            && !ui.get_reviewing()
            && !ui.get_followup_pending()
            && !ui.get_queue_pending()
            && !ui.get_attachment_loading()
        {
            let turn = ui.get_active_turn_id();
            let prompt = followup_attachments.prompt(text.clone());
            if turn.is_empty() || prompt.is_empty() {
                return;
            }
            let input = Input::Followup {
                conversation: ui.get_conversation_id(),
                turn: steer.then_some(turn),
                prompt,
            };
            ui.invoke_save_composer_draft();
            if dispatch(&mut ui, &commands, input) {
                ui.set_submitted_draft(text);
                ui.set_followup_pending(true);
            }
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_remove_queued(move |id| {
        if let Some(mut ui) = weak.upgrade()
            && !ui.get_queue_pending()
            && dispatch(&mut ui, &commands, Input::RemoveQueued(id))
        {
            ui.set_queue_pending(true);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_start_queued(move |id| {
        if let Some(mut ui) = weak.upgrade()
            && !ui.get_busy()
            && !ui.get_queue_pending()
            && dispatch(&mut ui, &commands, Input::StartQueued(id))
        {
            ui.set_queue_pending(true);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_resume_queue(move || {
        if let Some(mut ui) = weak.upgrade()
            && !ui.get_busy()
            && !ui.get_queue_pending()
            && dispatch(&mut ui, &commands, Input::ResumeQueue)
        {
            ui.set_queue_pending(true);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_send(move |draft| {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_connected()
            && !ui.get_busy()
            && !ui.get_switching()
            && !ui.get_conversation_archived()
            && !ui.get_archive_pending()
            && !ui.get_queue_pending()
            && !ui.get_followup_pending()
            && !ui.get_attachment_loading()
            && !ui.get_worktree_preparing()
            && (ui.get_queue_resumed()
                || !ui.get_queue_loading() && ui.model_queued_messages().is_empty())
            && !ui.get_workspace().is_empty()
        {
            let prompt = attachments.prompt(draft.clone());
            if prompt.is_empty() {
                return;
            }
            ui.invoke_save_composer_draft();
            if !ui.get_worktree_start_label().is_empty() {
                composer_worktrees.send(&mut ui, commands.clone(), prompt, draft);
                return;
            }
            if !dispatch(&mut ui, &commands, Input::Send(prompt)) {
                return;
            }
            ui.set_busy(true);
            show_status(&mut ui, Status::AwaitingReply);
            ui.set_submitted_draft(draft);
            ui.set_sending_prompt(true);
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_interrupt(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        let conversation = ui.get_conversation_id();
        if ui.get_busy()
            && !ui.get_switching()
            && !ui.get_stopping()
            && dispatch(&mut ui, &commands, Input::Interrupt(conversation))
        {
            ui.set_stopping(true);
            show_status(&mut ui, Status::Stopping);
        }
    });
}

fn can_request_fork(ui: &AgentWorkbench) -> bool {
    ui.get_connected()
        && ui.get_renamable()
        && !ui.get_busy()
        && !ui.get_switching()
        && !ui.get_rename_pending()
        && !ui.get_archive_pending()
        && !ui.get_followup_pending()
        && !ui.get_queue_pending()
        && !ui.get_fork_pending()
        && !ui.get_worktree_preparing()
        && !ui.get_conversation_archived()
}

fn historical_prompt(
    snapshot: &Snapshot,
    conversation: i64,
    index: i64,
) -> Option<(&str, &str, bool)> {
    if snapshot.view.conversation != conversation || snapshot.view.restoring {
        return None;
    }
    let index = usize::try_from(index).ok()?;
    let Entry::User(message) = snapshot.view.entries.get(index)?.as_ref() else {
        return None;
    };
    let text = message.prompt_text();
    if text.trim().is_empty() {
        return None;
    }
    let turn = snapshot
        .view
        .fork_points
        .iter()
        .find(|point| point.prompt == Some(index))
        .map_or("", |point| point.id.as_str());
    Some((text, turn, text != message.display))
}

fn copy_transcript_text(
    ui: &mut AgentWorkbench,
    clipboard: &mut Option<arboard::Clipboard>,
    index: i64,
    offset: i64,
    text: &str,
) {
    let result = (|| {
        let clipboard = match clipboard {
            Some(clipboard) => clipboard,
            None => clipboard.insert(arboard::Clipboard::new()?),
        };
        clipboard.set_text(text)
    })();
    ui.set_message_copy_index(index);
    ui.set_message_copy_offset(offset);
    ui.set_message_copy_status(if result.is_ok() {
        "已复制".into()
    } else {
        "复制失败".into()
    });
}

fn copyable_response(snapshot: &Snapshot, conversation: i64, index: i64) -> Option<&str> {
    if snapshot.view.conversation != conversation || snapshot.view.restoring {
        return None;
    }
    match snapshot
        .view
        .entries
        .get(usize::try_from(index).ok()?)?
        .as_ref()
    {
        Entry::Assistant {
            text,
            finalized: true,
        } if !text.trim().is_empty() => Some(text),
        _ => None,
    }
}

fn append_message_quote(ui: &mut AgentWorkbench, selected: &str) -> bool {
    if selected.trim().is_empty() {
        return false;
    }
    let mut draft = ui.get_draft();
    if !draft.is_empty() {
        draft.push_str("\n\n");
    }
    for (index, line) in selected.split('\n').enumerate() {
        if index != 0 {
            draft.push('\n');
        }
        draft.push_str("> ");
        draft.push_str(line);
    }
    draft.push_str("\n\n");
    ui.set_draft(draft);
    true
}

fn start(args: Args) -> Result<(), String> {
    let keep_launch_workspace = args.workspace.is_some();
    let workspace = args
        .workspace
        .or_else(|| env::var_os("BUILD_WORKING_DIRECTORY").map(PathBuf::from))
        .or_else(|| env::current_dir().ok())
        .map(|path| workspace_path(&path))
        .transpose()?
        .unwrap_or_default();
    let state_directory = env::var_os("CODEX_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::home_dir().map(|home| home.join(".codex")))
        .map(|home| home.join("xrack-workbench"));
    let worktree_base = state_directory
        .as_ref()
        .map(|directory| directory.join("worktrees"));
    let (client, output) = Client::start(
        &args.codex,
        workspace.clone(),
        args.workspace_write,
        state_directory.clone(),
        keep_launch_workspace,
    )
    .map_err(|error| format!("初始化 Codex 连接失败：{error}"))?;
    let mut ui = AgentWorkbench::new();
    ui.set_workspace_name(folder_name(&workspace));
    ui.set_workspace(workspace);
    ui.set_workspace_write(args.workspace_write);
    ui.set_worktree_available(worktree_base.is_some());
    appearance::attach(&mut ui, state_directory.as_deref());
    settings::attach_language(&mut ui, state_directory.as_deref());
    settings::attach_composer(&mut ui, state_directory.as_deref());
    external_open::attach(&mut ui, state_directory.as_deref(), output.clone());
    review::attach(&mut ui, output.clone());
    review_reports::attach(&mut ui, &client, output.clone());
    let notices = notifications::attach(&mut ui, state_directory.as_deref());
    attach(
        &mut ui,
        &client,
        output,
        worktree_base,
        notices,
        session::DraftStore::new(state_directory),
    );
    let result = run_with_viewport_and_system_color_scheme(
        ui,
        |ui, viewport| {
            ui.set_viewport_width(Length(f64::from(viewport.width)));
            ui.set_viewport_height(Length(f64::from(viewport.height)));
        },
        appearance::update_system,
    );
    client.shutdown();
    result
}

fn run_workbench() -> u8 {
    match start(Args::parse()) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

#[cfg(not(feature = "macos-bundle"))]
fn main() -> std::process::ExitCode {
    run_workbench().into()
}

#[cfg(feature = "macos-bundle")]
#[unsafe(export_name = "main")]
pub extern "C" fn macos_main() -> std::ffi::c_int {
    run_workbench().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use session::TurnDiff;

    fn update(ui: &mut AgentWorkbench, snapshot: &Snapshot, previous: Option<&Snapshot>) {
        super::update(
            ui,
            snapshot,
            previous,
            &mut file_references::TranscriptStream::default(),
        );
    }

    #[test]
    fn message_copy_keeps_markdown_and_quote_preserves_the_draft() {
        let mut snapshot = Snapshot::connecting("/workspace".into());
        snapshot.view.conversation = 5;
        snapshot.view.entries = vec![
            Arc::new(Entry::Assistant {
                text: "流式输出".into(),
                finalized: false,
            }),
            Arc::new(Entry::Assistant {
                text: "**第一行**\n\n第二行".into(),
                finalized: true,
            }),
        ];
        assert_eq!(
            copyable_response(&snapshot, 5, 1),
            Some("**第一行**\n\n第二行")
        );
        assert_eq!(copyable_response(&snapshot, 6, 1), None);
        assert_eq!(copyable_response(&snapshot, 5, 0), None);
        assert_eq!(copyable_response(&snapshot, 5, -1), None);

        let mut ui = AgentWorkbench::new();
        ui.set_draft("已有草稿".into());
        assert!(!append_message_quote(&mut ui, " \n"));
        assert!(append_message_quote(&mut ui, "第一行\n\n第二行"));
        assert_eq!(ui.get_draft(), "已有草稿\n\n> 第一行\n> \n> 第二行\n\n");
    }

    #[test]
    fn failed_followups_and_new_edits_keep_the_draft() {
        let mut ui = AgentWorkbench::new();
        let accepted = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let remembered = accepted.clone();
        ui.on_remember_composer_prompt(move |text| remembered.borrow_mut().push(text));
        let mut snapshot = Snapshot::connecting("/workspace".into());
        ui.set_draft("原稿".into());
        ui.set_submitted_draft("原稿".into());
        ui.set_followup_pending(true);
        snapshot.view.followup_error = "超时".into();
        update_followups(&mut ui, &snapshot, None);
        assert_eq!(ui.get_draft(), "原稿");
        assert!(!ui.get_followup_pending());
        assert!(accepted.borrow().is_empty());

        ui.set_followup_pending(true);
        ui.set_submitted_draft("原稿".into());
        ui.set_draft("原稿加上新的批注".into());
        snapshot.view.followup_error.clear();
        update_followups(&mut ui, &snapshot, None);
        assert_eq!(ui.get_draft(), "原稿加上新的批注");
        assert_eq!(accepted.borrow().as_slice(), ["原稿"]);
        ui.set_followup_pending(true);
        ui.set_submitted_draft(ui.get_draft());
        update_followups(&mut ui, &snapshot, None);
        assert!(ui.get_draft().is_empty());
    }

    #[test]
    fn an_autostarted_turn_does_not_clear_an_unaccepted_prompt() {
        let mut ui = AgentWorkbench::new();
        let previous = Snapshot::connecting("/workspace".into());
        let mut native = previous.clone();
        native.view.busy = true;
        ui.set_draft("下一条草稿".into());
        ui.set_submitted_draft("下一条草稿".into());
        ui.set_sending_prompt(true);
        update_followups(&mut ui, &native, Some(&previous));
        assert_eq!(ui.get_draft(), "下一条草稿");
        assert!(!ui.get_sending_prompt());

        native.view.sent_prompts += 1;
        ui.set_submitted_draft("下一条草稿".into());
        ui.set_sending_prompt(true);
        update_followups(&mut ui, &native, Some(&previous));
        assert!(ui.get_draft().is_empty());
    }

    #[test]
    fn sidebar_history_pages_and_new_sessions_keep_existing_rows() {
        let mut ui = AgentWorkbench::new();
        let model = ui.model_conversations();
        let prior = Snapshot::connecting("/workspace".into());
        update(&mut ui, &prior, None);
        let original = model.row_key(0).unwrap();
        let mut next = prior.clone();
        let mut stored = prior.conversations[0].clone();
        stored.id = 1;
        stored.title = "Saved".into();
        next.conversations.push(stored);
        update(&mut ui, &next, Some(&prior));
        assert_eq!(model.row_key(0), Some(original));
        let saved = model.row_key(1).unwrap();
        let mut added = next.clone();
        let mut new = prior.conversations[0].clone();
        new.id = 2;
        added.conversations.insert(0, new);
        update(&mut ui, &added, Some(&next));
        assert_eq!(model.row_key(1), Some(original));
        assert_eq!(model.row_key(2), Some(saved));

        let mut filtered = added.clone();
        filtered.history_query = "Saved".into();
        update(&mut ui, &filtered, Some(&added));
        assert!(!model.row(0).unwrap().shown);
        assert!(!model.row(1).unwrap().shown);
        assert!(!model.row(2).unwrap().shown);
        assert_eq!(model.row_key(2), Some(saved));
        assert_eq!(ui.get_history_hit_count(), 0);
    }

    #[test]
    fn a_failed_rename_keeps_the_dialog_open_and_input_intact() {
        let mut ui = AgentWorkbench::new();
        let mut snapshot = Snapshot::connecting("/workspace".into());
        snapshot.view.renamable = true;
        update(&mut ui, &snapshot, None);
        ui.set_rename_open(true);
        ui.set_rename_draft("Current attempt".into());
        ui.set_rename_pending(true);
        let mut failed = snapshot.clone();
        failed.view.rename_error = "Could not save".into();
        update(&mut ui, &failed, Some(&snapshot));
        assert!(ui.get_rename_open());
        assert!(!ui.get_rename_pending());
        assert_eq!(ui.get_rename_draft(), "Current attempt");
        assert_eq!(ui.get_rename_error(), "Could not save");
        ui.set_rename_pending(true);
        snapshot.conversations[0].title = "Current attempt".into();
        update(&mut ui, &snapshot, Some(&failed));
        assert!(!ui.get_rename_open());
        assert_eq!(ui.get_conversation_title(), "Current attempt");
    }

    #[test]
    fn earlier_turns_preserve_visible_row_keys_and_diff_selection() {
        let mut ui = AgentWorkbench::new();
        let model = ui.model_entries();
        let mut prior = Snapshot::connecting("/workspace".into());
        prior.view.entries = vec![
            Arc::new(Entry::User("recent".into())),
            Arc::new(Entry::Changes(TurnDiff::parse("diff --git a/new b/new\n"))),
        ];
        update(&mut ui, &prior, None);
        let prompt_key = model.row_key(0).unwrap();
        let change_key = model.row_key(1).unwrap();
        ui.set_review_turn(1);
        let mut next = prior.clone();
        next.view
            .entries
            .insert(0, Arc::new(Entry::User("earlier".into())));
        update(&mut ui, &next, Some(&prior));
        assert_eq!(model.row_key(1), Some(prompt_key));
        assert_eq!(model.row_key(2), Some(change_key));
        assert_eq!(ui.get_review_turn(), 2);
    }

    #[test]
    fn finalized_code_sections_keep_search_matches_and_identity_after_history_prepend() {
        let mut ui = AgentWorkbench::new();
        chat_find::attach(&mut ui);
        let mut snapshot = Snapshot::connecting("/workspace".into());
        snapshot.view.entries.push(Arc::new(Entry::Assistant {
            text: "见 [源码](src/lib.rs:7)\n\n```rust\nlet café = 7;\n```\n\n完成".into(),
            finalized: true,
        }));
        update(&mut ui, &snapshot, None);
        let owners = ui.get_transcript_section_owners();
        assert_eq!(owners.len(), 1);
        assert!(!owners[0].is_empty());
        let sections = ui.model_transcript_sections();
        assert_eq!(sections.len(), 3);
        let key = sections.row_key(1).unwrap();
        assert_eq!(
            sections.row(0).unwrap().body.links()[0].href,
            "workspace-file:src/lib.rs:7"
        );

        ui.set_chat_find_open(true);
        ui.set_chat_find_query("café".into());
        ui.invoke_refresh_chat_find();
        assert_eq!(ui.get_chat_find_count(), 1);
        let code = sections.row(1).unwrap();
        assert_eq!(code.body.text(), "let café = 7;\n");
        assert!(
            code.annotated
                .runs()
                .iter()
                .any(|run| run.style.underline && run.style.monospace)
        );
        assert!(code.body.runs().iter().all(|run| !run.style.underline));

        let mut page = snapshot.clone();
        page.view
            .entries
            .insert(0, Arc::new(Entry::User("上一轮".into())));
        update(&mut ui, &page, Some(&snapshot));
        assert_eq!(
            ui.get_transcript_section_owners(),
            [String::new(), owners[0].clone()]
        );
        assert_eq!(sections.row_key(1), Some(key));
        assert_eq!(ui.get_chat_find_count(), 1);

        let mut edited = page.clone();
        edited.view.entries[1] = Arc::new(Entry::Assistant {
            text: "只有正文".into(),
            finalized: true,
        });
        update(&mut ui, &edited, Some(&page));
        assert!(sections.is_empty());
        assert!(ui.get_transcript_section_owners().is_empty());
        assert_eq!(ui.get_chat_find_count(), 0);
    }

    #[test]
    fn command_groups_update_status_and_keep_a_search_match_after_prepend() {
        let mut ui = AgentWorkbench::new();
        chat_find::attach(&mut ui);
        let command = |name: &str, state, output: &str| {
            Arc::new(Entry::Command(session::CommandActivity {
                command: name.into(),
                state,
                exit_code: None,
                output: output.into(),
                output_truncated: false,
            }))
        };
        let mut initial = Snapshot::connecting("/workspace".into());
        initial.view.entries = vec![
            command("bazel build", ActivityState::Completed, "compiled"),
            command("bazel test", ActivityState::Running, "only in test output"),
            Arc::new(Entry::Assistant {
                text: "阶段总结".into(),
                finalized: true,
            }),
            command("git status --short", ActivityState::Completed, "clean"),
        ];
        update(&mut ui, &initial, None);
        let entries = ui.model_entries();
        let row_key = entries.row_key(1);
        let groups = ui.model_command_groups();
        assert_eq!(groups.row(0).unwrap().status, "1 进行中");
        assert_eq!(groups.row(1).unwrap().last, 1);
        assert_eq!(
            groups.row(3).unwrap(),
            workbench_ui::CommandGroup::default()
        );

        let mut failed = initial.clone();
        failed.view.entries[1] =
            command("bazel test", ActivityState::Failed, "only in test output");
        update(&mut ui, &failed, Some(&initial));
        let group = groups.row(0).unwrap();
        assert_eq!(
            (group.status.as_str(), group.status_tone.as_str()),
            ("1 失败", "error")
        );
        assert_eq!(entries.row_key(1), row_key);

        ui.set_expanded_command_group(0);
        ui.set_chat_find_open(true);
        ui.set_chat_find_query("only in test".into());
        ui.invoke_refresh_chat_find();
        assert_eq!(ui.get_expanded_command(), 1);
        let mut earlier = failed.clone();
        earlier
            .view
            .entries
            .insert(0, Arc::new(Entry::User("earlier".into())));
        update(&mut ui, &earlier, Some(&failed));
        assert_eq!(ui.get_expanded_command_group(), 1);
        assert_eq!(ui.get_expanded_command(), 2);
        assert_eq!(ui.get_chat_find_position(), 1);
        assert_eq!(entries.row_key(2), row_key);
        assert_eq!(groups.row(1).unwrap().status, "1 失败");
    }

    #[test]
    fn opening_another_conversation_restores_tail_following() {
        let mut ui = AgentWorkbench::new();
        let mut prior = Snapshot::connecting("/workspace".into());
        prior.view.conversation = 1;
        prior.view.entries = vec![Arc::new(Entry::User("reading".into()))];
        update(&mut ui, &prior, None);
        ui.set_follow_output(false);
        update(&mut ui, &prior, Some(&prior));
        assert!(!ui.get_follow_output());

        let mut next = prior.clone();
        next.view.conversation = 2;
        next.view.entries = vec![Arc::new(Entry::User("other conversation".into()))];
        update(&mut ui, &next, Some(&prior));
        assert!(ui.get_follow_output());
    }

    #[test]
    fn sidebar_projects_pinned_rows_once_and_keeps_their_identity_on_archive() {
        let mut ui = AgentWorkbench::new();
        let mut original = Snapshot::connecting("/workspace".into());
        original.conversations[0].pinned = true;
        update_conversations(&mut ui, &original, None);
        let rows = ui.model_conversations();
        let key = rows.row_key(0);
        let pinned = rows.row(0).unwrap();
        assert!(pinned.pinned && pinned.pinned_shown && !pinned.shown);
        assert_eq!(ui.get_pinned_count(), 1);

        let mut archived = original.clone();
        archived.archive_view = true;
        archived.conversations[0].archived = true;
        update_conversations(&mut ui, &archived, Some(&original));
        let row = rows.row(0).unwrap();
        assert!(row.shown && !row.pinned && !row.pinned_shown);
        assert_eq!(rows.row_key(0), key);
        assert_eq!(ui.get_pinned_count(), 0);
        update_conversations(&mut ui, &original, Some(&archived));
        assert_eq!(ui.get_pinned_count(), 1);
        assert_eq!(rows.row_key(0), key);
    }
}
