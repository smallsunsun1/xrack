use crate::{
    attachments::Attachments,
    palette::{FileIndex, file_item},
    read_in_background,
};
use session::{Commands, Input, Skill, SkillCatalogState, Snapshot};
use std::{
    cell::RefCell,
    ops::Range,
    path::Path,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use workbench_ui::AgentWorkbench;

const MAX_SUGGESTIONS: usize = 4;
const INDEX_FRESHNESS: Duration = Duration::from_secs(30);

#[derive(Debug, PartialEq, Eq)]
struct Mention {
    kind: MentionKind,
    range: Range<usize>,
    query: String,
}

#[derive(Debug, PartialEq, Eq)]
enum MentionKind {
    File,
    Skill,
}

fn token_boundary(character: char) -> bool {
    character.is_whitespace()
        || "()[]{}<>（）【】《》〈〉,;!?\u{3001}\u{3002}\u{ff0c}\u{ff1b}\u{ff01}\u{ff1f}\"'`"
            .contains(character)
}

fn mention_at(text: &str, caret: usize) -> Option<Mention> {
    let before = text.get(..caret)?;
    before
        .rmatch_indices(['@', '$'])
        .find_map(|(start, marker)| {
            if before[..start]
                .chars()
                .next_back()
                .is_some_and(|character| {
                    character.is_ascii_alphanumeric() || "_.-/\\@$".contains(character)
                })
            {
                return None;
            }
            let kind = if marker == "@" {
                MentionKind::File
            } else {
                MentionKind::Skill
            };
            let query = &before[start + 1..];
            let remaining = &text[caret..];
            let end = caret + remaining.find(token_boundary).unwrap_or(remaining.len());
            let value = &text[start + 1..end];
            if query.chars().any(token_boundary)
                || kind == MentionKind::Skill
                    && (value.starts_with(|character: char| character.is_ascii_digit())
                        || !value
                            .chars()
                            .all(|ch| ch.is_alphanumeric() || ch == '-' || ch == '_'))
            {
                return None;
            }
            Some(Mention {
                kind,
                range: start..end,
                query: query.to_owned(),
            })
        })
}

struct Request {
    workspace: String,
    checkout_revision: i64,
    conversation: i64,
    draft: String,
    caret: i64,
    mention: Mention,
}

impl Request {
    fn from_ui(ui: &AgentWorkbench, caret: i64) -> Option<Self> {
        let draft = ui.get_draft();
        let mention = mention_at(&draft, usize::try_from(caret).ok()?)?;
        Some(Self {
            workspace: ui.get_workspace(),
            checkout_revision: ui.get_git_checkout_revision(),
            conversation: ui.get_conversation_id(),
            draft,
            caret,
            mention,
        })
    }

    fn is_current(&self, ui: &AgentWorkbench) -> bool {
        ui.get_file_mention_current()
            && self.checkout_revision == ui.get_git_checkout_revision()
            && ui.get_workspace() == self.workspace
            && ui.get_conversation_id() == self.conversation
            && ui.get_file_mention_draft() == self.draft
            && ui.get_file_mention_caret() == self.caret
    }

    fn remove_from_draft(mut self, ui: &mut AgentWorkbench) {
        if self.is_current(ui) {
            let offset = self.mention.range.start as i64;
            self.draft.replace_range(self.mention.range, "");
            ui.set_draft(self.draft);
            ui.invoke_restore_composer_caret(offset);
        }
    }
}

struct CachedIndex {
    workspace: String,
    checkout_revision: i64,
    built_at: Instant,
    index: Arc<FileIndex>,
}

#[derive(Default)]
struct Search {
    pending: Option<Request>,
    working: bool,
    cached: Option<CachedIndex>,
}

async fn search_files(search: &RefCell<Search>, request: &Request) -> Result<Vec<String>, String> {
    let cached = search
        .borrow()
        .cached
        .as_ref()
        .filter(|cached| {
            cached.workspace == request.workspace
                && cached.checkout_revision == request.checkout_revision
                && cached.built_at.elapsed() < INDEX_FRESHNESS
        })
        .map(|cached| cached.index.clone());
    let index = match cached {
        Some(index) => index,
        None => {
            let workspace = request.workspace.clone();
            let index = read_in_background("workbench-mention-index", move || {
                FileIndex::build(Path::new(&workspace))
            })
            .await?;
            let index = Arc::new(index);
            search.borrow_mut().cached = Some(CachedIndex {
                workspace: request.workspace.clone(),
                checkout_revision: request.checkout_revision,
                built_at: Instant::now(),
                index: index.clone(),
            });
            index
        }
    };
    let query = request.mention.query.clone();
    read_in_background("workbench-mention-search", move || {
        Ok(index.search(&query, MAX_SUGGESTIONS))
    })
    .await
}

fn catalog_state<'a>(snapshot: &'a Snapshot, workspace: &str) -> Option<&'a SkillCatalogState> {
    snapshot
        .skills
        .as_deref()
        .filter(|catalog| catalog.workspace == workspace)
        .map(|catalog| &catalog.state)
}

fn show_skills(ui: &mut AgentWorkbench, state: &SkillCatalogState, query: &str) {
    let items = ui.model_file_mention_items();
    items.clear();
    ui.set_file_mention_index(0);
    ui.set_skill_mention_state(
        match state {
            SkillCatalogState::Loading => "loading",
            SkillCatalogState::Ready(_) => "ready",
            SkillCatalogState::Failed(_) => "error",
        }
        .into(),
    );
    ui.set_file_mention_status(match state {
        SkillCatalogState::Failed(error) => error.clone(),
        _ => String::new(),
    });
    if let SkillCatalogState::Ready(skills) = state {
        let query = query.to_lowercase();
        for skill in skills
            .iter()
            .filter(|s| s.name.to_lowercase().starts_with(&query))
            .chain(skills.iter().filter(|s| {
                let name = s.name.to_lowercase();
                !name.starts_with(&query) && name.contains(&query)
            }))
            .take(MAX_SUGGESTIONS)
        {
            let detail = if skills
                .iter()
                .filter(|other| other.name == skill.name)
                .take(2)
                .count()
                > 1
            {
                skill.path.clone()
            } else {
                skill.description.clone()
            };
            items.push(workbench_ui::PaletteItem {
                key: skill.path.clone(),
                title: format!("${}", skill.name),
                detail,
                enabled: true,
            });
        }
    }
}

pub(super) fn sync_skills(
    ui: &mut AgentWorkbench,
    snapshot: &Snapshot,
    previous: Option<&Snapshot>,
) {
    if !ui.get_skill_mention()
        || !ui.get_file_mention_open()
        || previous.is_some_and(|old| match (&old.skills, &snapshot.skills) {
            (Some(old), Some(new)) => Arc::ptr_eq(old, new),
            (None, None) => true,
            _ => false,
        })
    {
        return;
    }
    if let Some(request) = Request::from_ui(ui, ui.get_file_mention_caret())
        && request.mention.kind == MentionKind::Skill
        && request.is_current(ui)
        && let Some(state) = catalog_state(snapshot, &request.workspace)
    {
        show_skills(ui, state, &request.mention.query);
    } else {
        ui.invoke_dismiss_file_mention();
    }
}

pub(super) fn attach(
    ui: &mut AgentWorkbench,
    attachments: &Attachments,
    commands: Commands,
    output: tokio::sync::watch::Receiver<Snapshot>,
) {
    let weak = ui.as_weak();
    let search = Rc::new(RefCell::new(Search::default()));
    let skill_output = output.clone();
    ui.on_search_file_mention(move |caret, selected| {
        let Some(mut ui) = weak.upgrade() else { return };
        if selected || !ui.get_can_suggest_files() {
            return;
        }
        let Some(request) = Request::from_ui(&ui, caret) else {
            return;
        };
        let skill = request.mention.kind == MentionKind::Skill;
        if skill && !ui.get_connected() {
            return;
        }
        ui.set_file_mention_draft(request.draft.clone());
        ui.set_file_mention_caret(caret);
        ui.set_skill_mention(skill);
        ui.set_file_mention_open(true);
        if skill {
            let snapshot = skill_output.borrow();
            let workspace = &request.workspace;
            let query = &request.mention.query;
            match catalog_state(&snapshot, workspace) {
                Some(state @ (SkillCatalogState::Ready(_) | SkillCatalogState::Loading)) => {
                    show_skills(&mut ui, state, query);
                }
                _ => match commands.send(Input::LoadSkills(workspace.clone())) {
                    Ok(()) => show_skills(&mut ui, &SkillCatalogState::Loading, query),
                    Err(error) => show_skills(&mut ui, &SkillCatalogState::Failed(error), query),
                },
            }
            return;
        }
        ui.set_file_mention_status("正在搜索工作区文件…".into());
        ui.model_file_mention_items().clear();

        {
            let mut state = search.borrow_mut();
            state.pending = Some(request);
            if state.working {
                return;
            }
            state.working = true;
        }
        let search_task = search.clone();
        let weak = weak.clone();
        if let Err(error) = xrack_platform_winit::spawn_local(async move {
            loop {
                let request = search_task.borrow_mut().pending.take();
                let Some(request) = request else { break };
                let result = search_files(&search_task, &request).await;
                let Some(mut ui) = weak.upgrade() else { break };
                if !ui.get_file_mention_open() || !request.is_current(&ui) {
                    continue;
                }
                match result {
                    Ok(paths) => {
                        ui.set_file_mention_status(if paths.is_empty() {
                            "没有匹配的工作区文件".into()
                        } else {
                            String::new()
                        });
                        ui.set_file_mention_index(0);
                        let items = ui.model_file_mention_items();
                        items.clear();
                        for path in paths {
                            items.push(file_item(path));
                        }
                    }
                    Err(error) => ui.set_file_mention_status(format!("文件搜索失败（{error}）")),
                }
            }
            search_task.borrow_mut().working = false;
        }) {
            search.borrow_mut().working = false;
            ui.set_file_mention_status(error.to_string());
        }
    });

    let weak = ui.as_weak();
    let attachments = attachments.clone();
    ui.on_attach_file_mention(move |index| {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_file_mention_open() || !ui.get_can_suggest_files() {
            return;
        }
        let Some(request) = Request::from_ui(&ui, ui.get_file_mention_caret()) else {
            return;
        };
        if !request.is_current(&ui) {
            return;
        }
        let Some(item) = usize::try_from(index)
            .ok()
            .and_then(|row| ui.model_file_mention_items().row(row))
        else {
            return;
        };
        if request.mention.kind == MentionKind::Skill {
            let skill: Option<Skill> = match catalog_state(&output.borrow(), &request.workspace) {
                Some(SkillCatalogState::Ready(skills)) => {
                    skills.iter().find(|skill| skill.path == item.key).cloned()
                }
                _ => None,
            };
            if let Some(skill) = skill
                && attachments.add_skill(&mut ui, &skill)
            {
                ui.invoke_dismiss_file_mention();
                request.remove_from_draft(&mut ui);
            }
            return;
        }
        let path = Path::new(&request.workspace).join(item.key);
        let Some(path) = path.to_str() else { return };
        ui.invoke_dismiss_file_mention();
        attachments.add_workspace_mention(
            &mut ui,
            path.into(),
            request.workspace.clone(),
            move |ui| request.remove_from_draft(ui),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use xrack_runtime::Component;

    #[test]
    fn finds_the_active_utf8_token_and_preserves_surrounding_draft_bytes() {
        let draft = "请看（@接口-old.md），然后处理";
        let caret = "请看（@接口".len();
        let mention = mention_at(draft, caret).unwrap();
        assert_eq!(mention.query, "接口");
        // A closing punctuation mark is part of the draft, even if the caret is in the token.
        let mut after = draft.to_owned();
        after.replace_range(mention.range, "");
        assert_eq!(after, "请看（），然后处理");
        assert_eq!(mention_at("看 @", "看 @".len()).unwrap().query, "");
    }

    #[test]
    fn ignores_addresses_and_queries_outside_the_active_token() {
        for draft in [
            "email@domain.tld",
            "file/@data",
            "看看 @已完成 继续",
            "没提到文件",
        ] {
            assert!(mention_at(draft, draft.len()).is_none(), "{draft}");
        }
        assert!(mention_at("看 @文件", 1).is_none());
    }

    #[test]
    fn skill_search_handles_utf8_tokens_and_leaves_file_names_and_amounts_alone() {
        let draft = "用（$ui-check），然后运行";
        let caret = "用（$ui".len();
        let mention = mention_at(draft, caret).unwrap();
        assert_eq!(mention.kind, MentionKind::Skill);
        assert_eq!(&draft[mention.range], "$ui-check");
        assert_eq!(mention.query, "ui");
        for draft in ["预算 $50", "${HOME}", "not$skill", "/path/$name"] {
            assert!(mention_at(draft, draft.len()).is_none(), "{draft}");
        }
        let path = "看 @dir/$test";
        let mention = mention_at(path, path.len()).unwrap();
        assert_eq!(mention.kind, MentionKind::File);
        assert_eq!(mention.query, "dir/$test");
    }

    #[test]
    fn skill_results_apply_to_the_current_workspace_and_highlight_duplicate_paths() {
        let mut ui = AgentWorkbench::new();
        let workspace = "/workspace";
        let draft = "$check";
        ui.set_workspace(workspace.into());
        ui.set_draft(draft.into());
        ui.set_file_mention_draft(draft.into());
        ui.set_file_mention_caret(draft.len() as i64);
        ui.set_file_mention_open(true);
        ui.set_skill_mention(true);
        let scene = ui.render_shared();
        let focus = scene.find("input").unwrap().focus_handle.as_ref().unwrap();
        focus.publish(true, true);
        let caret = draft.len();
        focus.publish_text_selection(caret, caret..caret, "");

        let mut snapshot = Snapshot::connecting(workspace.into());
        snapshot.skills = Some(Arc::new(session::SkillCatalog {
            workspace: workspace.into(),
            state: SkillCatalogState::Ready(
                [
                    ("recheck", "/user/recheck/SKILL.md"),
                    ("check", "/user/check/SKILL.md"),
                    ("check", "/workspace/check/SKILL.md"),
                ]
                .into_iter()
                .map(|(name, path)| Skill {
                    name: name.into(),
                    path: path.into(),
                    description: "Verify".into(),
                })
                .collect(),
            ),
        }));
        sync_skills(&mut ui, &snapshot, None);
        let rows = ui.model_file_mention_items();
        assert_eq!(rows.row(0).unwrap().title, "$check");
        assert_eq!(rows.row(0).unwrap().detail, "/user/check/SKILL.md");
        assert_eq!(rows.row(1).unwrap().detail, "/workspace/check/SKILL.md");
        assert_eq!(rows.row(2).unwrap().title, "$recheck");
        ui.set_draft("编辑后的 $che".into());
        sync_skills(&mut ui, &snapshot, None);
        assert!(!ui.get_file_mention_open());
    }

    #[test]
    fn attachment_completion_keeps_edits_made_while_reading() {
        let mut ui = AgentWorkbench::new();
        let draft = "检查 @counter-old.xr 里的构建规则";
        let caret = "检查 @counter".len();
        ui.set_workspace("/workspace".into());
        ui.set_draft(draft.into());
        ui.set_file_mention_draft(draft.into());
        ui.set_file_mention_caret(caret as i64);
        let scene = ui.render_shared();
        let focus = scene.find("input").unwrap().focus_handle.as_ref().unwrap();
        focus.publish(true, true);
        focus.publish_text_selection(caret, caret..caret, "");

        let request = Request::from_ui(&ui, caret as i64).unwrap();
        assert!(request.is_current(&ui));
        ui.set_draft(format!("{draft}，也请查测试"));
        request.remove_from_draft(&mut ui);
        assert_eq!(ui.get_draft(), format!("{draft}，也请查测试"));

        ui.set_draft(draft.into());
        let request = Request::from_ui(&ui, caret as i64).unwrap();
        request.remove_from_draft(&mut ui);
        assert_eq!(ui.get_draft(), "检查  里的构建规则");
    }
}
