use workbench_ui::{AgentWorkbench, ReviewLine, ReviewPair};
use xrack_runtime::Length;

const ROW_HEIGHT: f64 = 23.0;

fn paired_rows(lines: &[ReviewLine]) -> Vec<ReviewPair> {
    let empty = ReviewLine {
        id: -1,
        ..ReviewLine::default()
    };
    let mut pairs = Vec::new();
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let flush = |pairs: &mut Vec<ReviewPair>,
                 removed: &mut Vec<&ReviewLine>,
                 added: &mut Vec<&ReviewLine>| {
        for index in 0..removed.len().max(added.len()) {
            let left = removed.get(index).copied();
            let right = added.get(index).copied();
            let id = [left, right]
                .into_iter()
                .flatten()
                .map(|line| line.id)
                .min()
                .unwrap_or(-1);
            pairs.push(ReviewPair {
                id,
                full: false,
                left: left.unwrap_or(&empty).clone(),
                right: right.unwrap_or(&empty).clone(),
            });
        }
        removed.clear();
        added.clear();
    };
    for line in lines {
        match line.kind.as_str() {
            "remove" => removed.push(line),
            "add" => added.push(line),
            _ => {
                flush(&mut pairs, &mut removed, &mut added);
                let full = line.old_line <= 0 && line.new_line <= 0;
                pairs.push(ReviewPair {
                    id: line.id,
                    full,
                    left: line.clone(),
                    right: if full { empty.clone() } else { line.clone() },
                });
            }
        }
    }
    flush(&mut pairs, &mut removed, &mut added);
    pairs
}

pub(super) fn sync_pairs(ui: &AgentWorkbench, lines: &[ReviewLine]) {
    let model = ui.model_review_pairs();
    if !lines
        .iter()
        .any(|line| line.old_line > 0 || line.new_line > 0)
    {
        model.clear();
        return;
    }
    let pairs = paired_rows(lines);
    let count = pairs.len();
    for (index, row) in pairs.into_iter().enumerate() {
        if let Some(previous) = model.row(index) {
            if previous != row {
                model.set_row(index, row);
            }
        } else {
            model.push(row);
        }
    }
    while model.len() > count {
        model.remove(model.len() - 1);
    }
}

pub(super) fn toggle(ui: &mut AgentWorkbench) {
    let pairs = ui.model_review_pairs();
    if pairs.is_empty() || ui.get_review_raw() || ui.get_switching() {
        return;
    }
    let offset = ui.get_review_offset().0.max(0.0);
    let slot = (offset / ROW_HEIGHT) as usize;
    let within_row = offset % ROW_HEIGHT;
    let split = ui.get_review_split();
    let destination = if split {
        pairs.row(slot).and_then(|row| usize::try_from(row.id).ok())
    } else {
        ui.model_review_lines().row(slot).and_then(|line| {
            pairs
                .snapshot()
                .iter()
                .position(|row| row.left.id == line.id || row.right.id == line.id)
        })
    };
    ui.set_review_split(!split);
    ui.set_review_offset(Length(
        destination.map_or(0.0, |slot| slot as f64 * ROW_HEIGHT + within_row),
    ));
    ui.invoke_schedule_review_find_reveal();
}

#[cfg(test)]
mod tests {
    use super::super::{numbered_lines, set_patch};
    use super::*;

    const PATCH: &str = "未暂存\n@@ -5,3 +8,2 @@\n-old first\n-old second\n+new first\n shared\n@@ -20 +30 @@\n-old third\n+new third\n";

    #[test]
    fn aligns_changed_runs_and_preserves_unified_hunk_indices() {
        let lines = numbered_lines(PATCH);
        let rows = paired_rows(&lines);
        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            [0, 1, 2, 3, 5, 6, 7]
        );
        assert!(rows[1].full && rows[5].full);
        assert_eq!(rows[1].left.section, "未暂存");
        assert_eq!(
            (&*rows[2].left.content, rows[2].left.old_line),
            ("old first", 5)
        );
        assert_eq!(
            (&*rows[2].right.content, rows[2].right.new_line),
            ("new first", 8)
        );
        assert_eq!(rows[3].right.id, -1);
        assert_eq!((rows[4].left.old_line, rows[4].right.new_line), (7, 9));
        assert_eq!(rows[4].left.id, rows[4].right.id);
        assert_eq!(rows[6].right.id, 8);
    }

    #[test]
    fn switches_views_around_the_same_patch_row() {
        let mut ui = AgentWorkbench::new();
        set_patch(&mut ui, PATCH.into());
        ui.set_review_offset(Length(4.0 * ROW_HEIGHT + 3.0));
        toggle(&mut ui);
        assert!(ui.get_review_split());
        assert_eq!(ui.get_review_offset(), Length(2.0 * ROW_HEIGHT + 3.0));
        ui.set_review_offset(Length(5.0 * ROW_HEIGHT + 5.0));
        toggle(&mut ui);
        assert!(!ui.get_review_split());
        assert_eq!(ui.get_review_offset(), Length(6.0 * ROW_HEIGHT + 5.0));
    }

    #[test]
    fn metadata_only_patch_does_not_keep_cells_from_the_previous_file() {
        let mut ui = AgentWorkbench::new();
        set_patch(&mut ui, PATCH.into());
        assert!(!ui.model_review_pairs().is_empty());
        set_patch(
            &mut ui,
            "Binary files a/logo.png and b/logo.png differ\n".into(),
        );
        assert!(ui.model_review_pairs().is_empty());
        assert_eq!(ui.model_review_lines().row(0).unwrap().kind, "meta");
    }
}
