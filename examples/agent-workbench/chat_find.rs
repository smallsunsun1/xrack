use std::{cell::RefCell, collections::HashMap, ops::Range, rc::Rc};
use workbench_ui::{AgentWorkbench, ChatFindDocument, TranscriptEntry};
use xrack_runtime::RowKey;

#[derive(Clone, PartialEq, Eq)]
struct Hit {
    entry: RowKey,
    row: usize,
    bytes: Range<usize>,
}

#[derive(Default)]
struct Search {
    query: String,
    hits: Vec<Hit>,
    selected: usize,
}

impl Search {
    fn refresh(&mut self, ui: &mut AgentWorkbench) {
        if !ui.get_chat_find_open() {
            return;
        }
        let query = ui.get_chat_find_query();
        let model = ui.model_entries();
        let rows = model.snapshot();
        let old = self.hits.get(self.selected).cloned();
        let same_query = self.query == query;
        self.query = query;
        self.hits.clear();
        for (index, row) in rows.iter().enumerate() {
            let Some(entry) = model.row_key(index) else {
                continue;
            };
            self.hits.extend(
                transcript_search::matches(row.body.text(), &self.query)
                    .into_iter()
                    .map(|bytes| Hit {
                        entry,
                        row: index,
                        bytes,
                    }),
            );
        }
        let retained = old.filter(|_| same_query).and_then(|old| {
            self.hits
                .iter()
                .position(|hit| hit.entry == old.entry && hit.bytes == old.bytes)
        });
        self.selected = retained.unwrap_or(0);
        self.present(ui, &rows);
        if retained.is_none()
            || self
                .hits
                .get(self.selected)
                .is_some_and(|hit| crate::turn_activity::reveal(ui, hit.row))
        {
            self.schedule_reveal(ui);
        }
    }

    fn move_to(&mut self, ui: &mut AgentWorkbench, step: i64) {
        if !ui.get_chat_find_open() {
            return;
        }
        if ui.get_chat_find_query() != self.query {
            self.refresh(ui);
            return;
        }
        if self.hits.is_empty() {
            return;
        }
        self.selected =
            (self.selected as i64 + step.signum()).rem_euclid(self.hits.len() as i64) as usize;
        self.present(ui, &ui.model_entries().snapshot());
        self.schedule_reveal(ui);
    }

    fn present(&self, ui: &mut AgentWorkbench, rows: &[TranscriptEntry]) {
        ui.set_chat_find_result_query(self.query.clone());
        ui.set_chat_find_count(self.hits.len() as i64);
        ui.set_chat_find_position(if self.hits.is_empty() {
            0
        } else {
            self.selected as i64 + 1
        });
        let decoration = ui.model_chat_find_documents();
        if self.hits.is_empty() {
            decoration.clear();
            return;
        }
        let mut hits = self.hits.iter().peekable();
        let dark = ui.get_dark_appearance();
        let selected = self.hits.get(self.selected);
        for (index, row) in rows.iter().enumerate() {
            let mut ranges = Vec::new();
            while let Some(hit) = hits.next_if(|hit| hit.row == index) {
                ranges.push(hit.bytes.clone());
            }
            let active = selected
                .filter(|hit| hit.row == index)
                .map(|hit| &hit.bytes);
            let body = transcript_search::highlight(&row.body, &ranges, active, dark);
            let annotated = ChatFindDocument { body };
            if let Some(existing) = decoration.row(index) {
                if existing.body != annotated.body {
                    decoration.set_row(index, annotated);
                }
            } else {
                decoration.push(annotated);
            }
        }
        while decoration.len() > rows.len() {
            decoration.remove(decoration.len() - 1);
        }
        let sections = ui.model_transcript_sections();
        if sections.is_empty() {
            return;
        }
        let owners = ui.get_transcript_section_owners();
        let rows_by_owner: HashMap<_, _> = owners
            .iter()
            .enumerate()
            .filter(|(_, owner)| !owner.is_empty())
            .map(|(index, owner)| (owner.as_str(), index))
            .collect();
        for (index, mut section) in sections.snapshot().into_iter().enumerate() {
            let Some(document) = rows_by_owner
                .get(section.owner.as_str())
                .and_then(|row| decoration.row(*row))
            else {
                continue;
            };
            let start = section.start as usize;
            if let Some(styled) =
                transcript_search::slice(&document.body, start..start + section.body.text().len())
                && section.annotated != styled
            {
                section.annotated = styled;
                sections.set_row(index, section);
            }
        }
    }

    fn schedule_reveal(&self, ui: &mut AgentWorkbench) {
        let Some(hit) = self.hits.get(self.selected) else {
            return;
        };
        crate::turn_activity::reveal(ui, hit.row);
        if ui
            .model_entries()
            .row(hit.row)
            .is_some_and(|row| row.kind == "command")
        {
            ui.set_expanded_command(hit.row as i64);
        }
        ui.invoke_schedule_chat_find_reveal();
    }
}

pub(super) fn attach(ui: &mut AgentWorkbench) {
    let state = Rc::new(RefCell::new(Search::default()));
    let weak = ui.as_weak();
    let refresh = state.clone();
    ui.on_refresh_chat_find(move || {
        if let Some(mut ui) = weak.upgrade() {
            refresh.borrow_mut().refresh(&mut ui);
        }
    });

    let weak = ui.as_weak();
    let movement = state.clone();
    ui.on_move_chat_find(move |step| {
        if let Some(mut ui) = weak.upgrade() {
            movement.borrow_mut().move_to(&mut ui, step);
        }
    });

    let weak = ui.as_weak();
    ui.on_reveal_chat_find(move || {
        let state = state.borrow();
        if let Some(ui) = weak.upgrade()
            && let Some(hit) = state.hits.get(state.selected)
        {
            reveal_hit(&ui, hit);
        }
    });
}

fn reveal_hit(ui: &AgentWorkbench, hit: &Hit) {
    let owners = ui.get_transcript_section_owners();
    if let Some(owner) = owners.get(hit.row).filter(|owner| !owner.is_empty()) {
        let sections = ui.model_transcript_sections();
        for (index, section) in sections
            .snapshot()
            .into_iter()
            .filter(|section| &section.owner == owner)
            .enumerate()
        {
            let start = section.start as usize;
            if (start..start + section.body.text().len()).contains(&hit.bytes.start) {
                transcript_search::reveal_section(ui, hit.row, index, hit.bytes.start - start);
                return;
            }
        }
    } else {
        transcript_search::reveal(ui, hit.row, hit.bytes.start);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_a_match_survives_history_prepend_and_recomputes_a_streamed_row() {
        let mut ui = AgentWorkbench::new();
        attach(&mut ui);
        let entry = |kind: &str, text: &str| TranscriptEntry {
            kind: kind.into(),
            heading: String::new(),
            status: String::new(),
            status_tone: String::new(),
            body: xrack_runtime::StyledText::from_plain_text(text),
        };
        let model = ui.model_entries();
        model.push(entry("assistant", "Bazel first"));
        model.push(entry("command", "bazel test"));
        ui.set_chat_find_open(true);
        ui.set_chat_find_query("bazel".into());
        ui.invoke_refresh_chat_find();
        assert_eq!(ui.get_chat_find_count(), 2);
        ui.invoke_move_chat_find(1);
        assert_eq!(ui.get_chat_find_position(), 2);
        assert_eq!(ui.get_expanded_command(), 1);
        let command = ui.model_chat_find_documents().row(1).unwrap().body;

        model.insert(0, entry("assistant", "early Bazel result"));
        ui.invoke_refresh_chat_find();
        assert_eq!(ui.get_chat_find_count(), 3);
        assert_eq!(ui.get_chat_find_position(), 3);
        assert_eq!(ui.model_chat_find_documents().row(2).unwrap().body, command);

        assert!(model.set_row(2, entry("command", "finished with no results")));
        ui.invoke_refresh_chat_find();
        assert_eq!(ui.get_chat_find_count(), 2);
        assert_eq!(ui.get_chat_find_position(), 1);
        assert!(!ui.get_follow_output());
    }
}
