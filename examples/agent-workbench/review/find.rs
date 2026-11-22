use std::{cell::RefCell, ops::Range, rc::Rc};
use workbench_ui::{AgentWorkbench, ReviewLine, ReviewMatchRow};
use xrack_runtime::{Component, ScrollAction, ScrollAlignment, StyledText};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Hit {
    row: usize,
    raw: Range<usize>,
    rendered: Range<usize>,
}

#[derive(Default)]
struct Find {
    query: String,
    source: StyledText,
    hits: Vec<Hit>,
    current: usize,
}

fn prefix(line: &ReviewLine) -> &'static str {
    match line.kind.as_str() {
        "add" => "+ ",
        "remove" => "− ",
        "context" => "  ",
        _ => "",
    }
}

fn collect_hits(raw: &str, lines: &[ReviewLine], query: &str) -> Vec<Hit> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let mut base = 0;
    let mut hits = Vec::new();
    for (row, raw_line) in raw.split_inclusive('\n').enumerate() {
        let line = raw_line
            .strip_suffix('\n')
            .map_or(raw_line, |line| line.strip_suffix('\r').unwrap_or(line));
        let display_prefix = lines.get(row).map_or("", prefix);
        let mark = usize::from(!display_prefix.is_empty());
        if let Some(contents) = line.get(mark..) {
            let display_offset = |position| {
                display_prefix.len()
                    + position
                    + contents.as_bytes()[..position]
                        .iter()
                        .filter(|byte| **byte == b'\t')
                        .count()
                        * 3
            };
            for matched in transcript_search::matches(contents, query) {
                hits.push(Hit {
                    row,
                    raw: base + mark + matched.start..base + mark + matched.end,
                    rendered: display_offset(matched.start)..display_offset(matched.end),
                });
            }
        }
        base += raw_line.len();
    }
    hits
}

impl Find {
    fn refresh(&mut self, ui: &mut AgentWorkbench) {
        if !ui.get_review_find_open() {
            return;
        }
        let query = ui.get_review_find_query();
        let source = ui.get_review_body();
        let previous = if query == self.query && self.source.text() == source.text() {
            self.hits.get(self.current).cloned()
        } else {
            None
        };
        let lines = ui.model_review_lines().snapshot();
        self.hits = collect_hits(source.text(), &lines, &query);
        self.current = previous
            .and_then(|hit| self.hits.iter().position(|candidate| *candidate == hit))
            .unwrap_or(0);
        self.query = query;
        self.source = source;
        self.present(ui, &lines);
    }

    fn advance(&mut self, ui: &mut AgentWorkbench, step: i64) {
        if !ui.get_review_find_open()
            || self.hits.is_empty()
            || ui.get_review_find_query() != self.query
        {
            return;
        }
        self.current =
            (self.current as i64 + step.signum()).rem_euclid(self.hits.len() as i64) as usize;
        self.present(ui, &ui.model_review_lines().snapshot());
    }

    fn present(&self, ui: &mut AgentWorkbench, lines: &[ReviewLine]) {
        ui.set_review_find_result_query(self.query.clone());
        ui.set_review_find_count(self.hits.len() as i64);
        let current = self.hits.get(self.current);
        ui.set_review_find_position(current.map_or(0, |_| self.current as i64 + 1));
        let model = ui.model_review_match_rows();
        let Some(current) = current else {
            model.clear();
            ui.set_review_find_raw_body(StyledText::default());
            return;
        };
        let dark = ui.get_dark_appearance();
        let ranges: Vec<_> = self.hits.iter().map(|hit| hit.raw.clone()).collect();
        ui.set_review_find_raw_body(transcript_search::highlight(
            &self.source,
            &ranges,
            Some(&current.raw),
            dark,
        ));
        let mut hits = self.hits.iter().peekable();
        for (index, line) in lines.iter().enumerate() {
            let ranges: Vec<_> = std::iter::from_fn(|| hits.next_if(|hit| hit.row == index))
                .map(|hit| hit.rendered.clone())
                .collect();
            let mut row = ReviewMatchRow::default();
            if !ranges.is_empty() {
                row.marked = true;
                row.current = current.row == index;
                let plain =
                    StyledText::from_plain_text(format!("{}{}", prefix(line), line.content));
                row.body = transcript_search::highlight(
                    &plain,
                    &ranges,
                    row.current.then_some(&current.rendered),
                    dark,
                );
            }
            match model.row(index) {
                Some(previous) if previous != row => {
                    model.set_row(index, row);
                }
                None => {
                    model.push(row);
                }
                _ => {}
            }
        }
        while model.len() > lines.len() {
            model.remove(model.len() - 1);
        }
        ui.invoke_schedule_review_find_reveal();
    }

    fn reveal(&self, ui: &AgentWorkbench) {
        let Some(hit) = self
            .hits
            .get(self.current)
            .filter(|_| ui.get_review_find_query() == self.query)
        else {
            return;
        };
        let (element, action) =
            if ui.get_review_raw() || ui.model_review_lines().is_empty() {
                (
                    "patch-scroll",
                    ScrollAction::ToStyledText {
                        paragraph: 0,
                        offset: hit.raw.start,
                    },
                )
            } else if ui.get_review_split() && !ui.model_review_pairs().is_empty() {
                let Some(index) = ui.model_review_pairs().snapshot().iter().position(|pair| {
                    pair.left.id == hit.row as i64 || pair.right.id == hit.row as i64
                }) else {
                    return;
                };
                (
                    "review-pair-list",
                    ScrollAction::ToIndex {
                        index: index as i64,
                        alignment: ScrollAlignment::Center,
                    },
                )
            } else {
                (
                    "review-line-list",
                    ScrollAction::ToIndex {
                        index: hit.row as i64,
                        alignment: ScrollAlignment::Center,
                    },
                )
            };
        let scene = ui.render_shared();
        if let Some(port) = scene
            .find(element)
            .and_then(|node| node.viewport_handle.as_ref())
        {
            port.request(action);
        }
    }
}

pub(super) fn attach(ui: &mut AgentWorkbench) {
    let find = Rc::new(RefCell::new(Find::default()));
    let weak = ui.as_weak();
    let refresh = find.clone();
    ui.on_refresh_review_find(move || {
        if let Some(mut ui) = weak.upgrade() {
            refresh.borrow_mut().refresh(&mut ui);
        }
    });
    let weak = ui.as_weak();
    let advance = find.clone();
    ui.on_move_review_find(move |step| {
        if let Some(mut ui) = weak.upgrade() {
            advance.borrow_mut().advance(&mut ui, step);
        }
    });
    let weak = ui.as_weak();
    ui.on_reveal_review_find(move || {
        if let Some(ui) = weak.upgrade() {
            find.borrow().reveal(&ui);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::{numbered_lines, set_patch, split};
    use super::*;
    use std::time::Duration;
    use xrack_runtime::{EventResult, KeyEvent, KeyPhase, Length, Orientation};
    use xrack_testing::{Size, UiDriver};

    #[test]
    fn matches_share_raw_and_rendered_offsets_with_unicode_and_tabs() {
        let patch = "@@ -5,2 +7,2 @@\r\n-\tCafé\r\n+\tCAFÉ\r\n café\r\n";
        let lines = numbered_lines(patch);
        let hits = collect_hits(patch, &lines, "CAfé");
        assert_eq!(
            hits.iter().map(|hit| hit.row).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        for hit in hits {
            let rendered = format!("{}{}", prefix(&lines[hit.row]), lines[hit.row].content);
            assert_eq!(&rendered[hit.rendered], &patch[hit.raw]);
        }
    }

    #[test]
    fn find_reveals_the_same_match_in_numbered_split_and_raw_views() {
        let mut ui = AgentWorkbench::new();
        attach(&mut ui);
        let weak = ui.as_weak();
        ui.on_toggle_review_split(move || split::toggle(&mut weak.upgrade().unwrap()));
        ui.set_workspace("/workspace".into());
        ui.set_review_open(true);
        ui.set_viewport_width(Length(760.0));
        ui.set_review_selection("src/lib.rs".into());
        let patch = format!(
            "@@ -1,61 +1,61 @@\n{}-Needle before\n+needle after\n",
            " context\n".repeat(60)
        );
        set_patch(&mut ui, patch);
        let mut driver = UiDriver::headless(ui, Size::new(760.0, 560.0));
        assert!(driver.menu_shortcut("Primary+F".parse().unwrap()));
        assert_eq!(driver.focused(), Some("review-find-input"));
        driver.fill_focused("needle");
        driver.advance_time(Duration::from_millis(75));
        driver.advance_time(Duration::from_millis(3));
        assert_eq!(driver.component().get_review_find_count(), 2);
        assert_eq!(driver.component().get_review_find_position(), 1);
        let offset = |driver: &UiDriver<AgentWorkbench>, id| {
            driver
                .layout_at(&driver.path(id))
                .unwrap()
                .scroll_on(Orientation::Vertical)
                .unwrap()
                .offset
        };
        assert!(offset(&driver, "review-line-list") > 180.0);
        driver.click("review-find-next");
        driver.advance_time(Duration::from_millis(3));
        assert_eq!(driver.component().get_review_find_position(), 2);

        driver.click("split-patch");
        driver.advance_time(Duration::from_millis(3));
        assert!(driver.node("review-pair-list").is_visible());
        assert!(offset(&driver, "review-pair-list") > 180.0);
        assert_eq!(driver.component().get_review_find_position(), 2);

        driver.click("raw-patch");
        driver.advance_time(Duration::from_millis(3));
        assert!(driver.node("patch-scroll").is_visible());
        assert!(offset(&driver, "patch-scroll") > 180.0);
        assert_eq!(driver.component().get_review_find_position(), 2);
        assert_eq!(driver.component().get_review_find_count(), 2);
        let body = driver.node("review-diff").styled_text("text").unwrap();
        assert!(body.runs().iter().any(|run| run.style.underline));

        driver.click("close-review-find-button");
        let text = "Needle before";
        let body = driver.component().get_review_body();
        let start = body.text().find(text).unwrap();
        let end = start + text.len();
        let focus = driver
            .node("review-diff")
            .focus_handle
            .as_ref()
            .unwrap()
            .clone();
        focus.publish_text_selection(end, start..end, text);
        driver.update(|_| {});
        assert!(driver.menu_shortcut("Primary+F".parse().unwrap()));
        assert_eq!(driver.component().get_review_find_query(), text);
        assert_eq!(driver.component().get_review_find_count(), 1);
        let scope = driver.path("review-focus-scope");
        let scene = driver.scene().clone();
        driver.update(|view| {
            assert_eq!(
                view.key_event_at(
                    &scene,
                    &scope,
                    KeyPhase::CapturePressed,
                    &KeyEvent {
                        key: "Escape".into(),
                        ..KeyEvent::default()
                    },
                ),
                EventResult::Accept
            );
        });
        assert!(!driver.component().get_review_find_open());
        assert!(driver.component().get_review_open());
    }
}
