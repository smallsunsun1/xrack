use super::NoteDraft;
use session::SavedReviewNote;
use std::collections::HashSet;
use workbench_ui::{AgentWorkbench, ReviewNoteRow};

pub(crate) fn capture_notes(ui: &AgentWorkbench) -> Vec<SavedReviewNote> {
    ui.model_review_notes()
        .snapshot()
        .into_iter()
        .map(|row| SavedReviewNote {
            id: row.id,
            reference: row.reference,
            text: row.text,
        })
        .collect()
}

pub(crate) fn restore_notes(ui: &AgentWorkbench, items: Vec<SavedReviewNote>) {
    let model = ui.model_review_notes();
    model.clear();
    let mut seen = HashSet::new();
    for item in items {
        if item.id >= 0
            && !item.reference.trim().is_empty()
            && !item.text.trim().is_empty()
            && seen.insert(item.id)
        {
            model.push(ReviewNoteRow {
                id: item.id,
                reference: item.reference,
                text: item.text,
            });
        }
    }
}

pub(super) fn append<'a>(
    ui: &mut AgentWorkbench,
    items: impl IntoIterator<Item = (&'a str, &'a str)>,
) {
    let mut draft = ui.get_draft();
    for (reference, text) in items {
        if !draft.is_empty() {
            draft.push_str("\n\n");
        }
        draft.push_str(&format!("审阅批注（{reference}）\n{text}"));
    }
    ui.set_draft(draft);
}

pub(super) fn remove(ui: &mut AgentWorkbench, id: i64) {
    let model = ui.model_review_notes();
    if let Some(index) = model.snapshot().iter().position(|row| row.id == id) {
        model.remove(index);
        ui.invoke_draft_review_notes_changed();
    }
}

fn stage(ui: &mut AgentWorkbench) {
    if !ui.get_review_open() || ui.get_switching() {
        return;
    }
    let note = NoteDraft::capture(ui);
    let text = note.text.trim();
    if note.is_empty() || text.is_empty() {
        return;
    }
    let model = ui.model_review_notes();
    let rows = model.snapshot();
    let replace = note
        .queued
        .and_then(|id| rows.iter().position(|row| row.id == id));
    let id = if let Some(existing) = replace {
        rows[existing].id
    } else {
        let Some(id) = rows
            .iter()
            .map(|row| row.id)
            .max()
            .unwrap_or(-1)
            .checked_add(1)
        else {
            return;
        };
        id
    };
    let row = ReviewNoteRow {
        id,
        reference: note.reference,
        text: text.into(),
    };
    if let Some(index) = replace {
        model.set_row(index, row);
    } else {
        model.push(row);
    }
    NoteDraft::default().restore(ui);
    ui.invoke_draft_review_notes_changed();
}

fn edit(ui: &mut AgentWorkbench, id: i64) {
    if !ui.get_review_notes_open()
        || !ui.get_review_note_text().trim().is_empty()
        || ui.get_switching()
    {
        return;
    }
    let Some(note) = ui
        .model_review_notes()
        .snapshot()
        .into_iter()
        .find(|row| row.id == id)
    else {
        return;
    };
    NoteDraft {
        reference: note.reference,
        text: note.text,
        queued: Some(note.id),
    }
    .restore(ui);
    ui.set_review_notes_open(false);
    ui.invoke_focus_review_note();
}

fn apply(ui: &mut AgentWorkbench) {
    if !ui.get_review_notes_open()
        || ui.get_switching()
        || ui.get_conversation_archived()
        || ui.get_review_note_queued_id() >= 0
    {
        return;
    }
    let model = ui.model_review_notes();
    let rows = model.snapshot();
    if rows.is_empty() {
        return;
    }
    append(
        ui,
        rows.iter()
            .map(|row| (row.reference.as_str(), row.text.as_str())),
    );
    model.clear();
    ui.set_review_notes_open(false);
    ui.invoke_draft_review_notes_changed();
    ui.invoke_focus_composer();
}

pub(super) fn attach(ui: &mut AgentWorkbench) {
    let weak = ui.as_weak();
    ui.on_stage_review_note(move || {
        if let Some(mut ui) = weak.upgrade() {
            stage(&mut ui);
        }
    });
    let weak = ui.as_weak();
    ui.on_edit_staged_review_note(move |id| {
        if let Some(mut ui) = weak.upgrade() {
            edit(&mut ui, id);
        }
    });
    let weak = ui.as_weak();
    ui.on_remove_staged_review_note(move |id| {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_review_notes_open()
            && ui.get_review_note_queued_id() != id
            && !ui.get_switching()
        {
            remove(&mut ui, id);
        }
    });
    let weak = ui.as_weak();
    ui.on_apply_staged_review_notes(move || {
        if let Some(mut ui) = weak.upgrade() {
            apply(&mut ui);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_notes_can_be_edited_removed_and_applied_without_sending() {
        let mut ui = AgentWorkbench::new();
        attach(&mut ui);
        ui.set_review_open(true);
        ui.set_draft("先运行现有测试。".into());
        for (reference, text) in [
            ("src/one.rs · 旧行 11 · 未暂存", "  请保留错误分支  "),
            ("src/two.rs · 新行 23 · 未暂存", "删除这条"),
            ("src/three.rs · 新行 42 · 已暂存", "补充边界测试"),
        ] {
            ui.set_review_note_reference(reference.into());
            ui.set_review_note_text(text.into());
            ui.invoke_stage_review_note();
            assert_eq!(ui.get_draft(), "先运行现有测试。");
        }
        let model = ui.model_review_notes();
        assert_eq!(model.len(), 3);
        let first_key = model.row_key(0).unwrap();
        assert!(ui.get_review_note_reference().is_empty());
        ui.set_review_notes_open(true);
        ui.invoke_edit_staged_review_note(0);
        assert_eq!(ui.get_review_note_queued_id(), 0);
        assert!(!ui.get_review_notes_open());
        ui.set_review_note_text("保留错误分支并加一项检查".into());
        ui.invoke_stage_review_note();
        assert_eq!(model.row_key(0), Some(first_key));
        assert_eq!(model.len(), 3);
        ui.set_review_notes_open(true);
        ui.invoke_remove_staged_review_note(1);
        assert_eq!(
            model
                .snapshot()
                .iter()
                .map(|row| row.id)
                .collect::<Vec<_>>(),
            [0, 2]
        );
        ui.invoke_apply_staged_review_notes();
        assert_eq!(
            ui.get_draft(),
            "先运行现有测试。\n\n审阅批注（src/one.rs · 旧行 11 · 未暂存）\n保留错误分支并加一项检查\n\n审阅批注（src/three.rs · 新行 42 · 已暂存）\n补充边界测试",
        );
        assert!(model.is_empty());
        assert!(!ui.get_review_notes_open());
        assert!(!ui.get_sending_prompt());
    }
}
