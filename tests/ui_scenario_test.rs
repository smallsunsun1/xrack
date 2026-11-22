mod table_workload;

use std::time::Duration;
use table_workload::{GRID, ROW_COUNT, VIEWPORT, aot, interpreted, scroll, update_visible};
use xrack_interpreter::HostValue;
use xrack_runtime::{Component, VecModel};
use xrack_testing::{ActivationKey, TableEditCommit, TargetError, UiDriver};

#[test]
fn module_package_can_move_with_its_sources_resources_and_aot_output()
-> Result<(), Box<dyn std::error::Error>> {
    use std::{fs, path::PathBuf};

    let root = std::env::var_os("TEST_TMPDIR")
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join(format!("xrack-module-{}", std::process::id()));
    fs::create_dir(&root)?;
    let source = root.join("source");
    fs::create_dir(&source)?;
    fs::write(
        source.join("icon.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="red"/></svg>"#,
    )?;
    fs::write(
        source.join("body.xr"),
        r#"
        export component Card inherits Rectangle {
            Image { source: @image-url("icon.svg"); }
        }
    "#,
    )?;
    let entry = source.join("library.xr");
    fs::write(
        &entry,
        r#"
        import { Card } from "body.xr";
        export component Badge inherits Card {
            out property <int> answer: 42;
        }
    "#,
    )?;
    let staging = root.join("build");
    let manifest = xrack_compiler::pack_module(&entry, &staging, None, None)?;
    assert_eq!(manifest.components, ["Badge"]);
    assert!(staging.join(&manifest.rust).is_file());
    let installed = root.join("installed");
    fs::rename(&staging, &installed)?;
    fs::remove_dir_all(&source)?;

    let program = xrack_interpreter::load_module_package(&installed)?;
    let instance = program.create("Badge")?;
    assert_eq!(instance.get("answer")?, HostValue::Int(42));
    assert!(
        instance.render().children[0]
            .image_asset("source")
            .is_some()
    );
    let importer = r#"
        import { Badge as AppBadge } from "installed/xrack-module.json";
        export component UI inherits Window { AppBadge {} }
    "#;
    let dependent = root.join("app.xr");
    fs::write(&dependent, importer)?;
    let compiled = xrack_compiler::compile(importer, &dependent.to_string_lossy())?;
    assert!(compiled.rust.to_string().contains("UI"));
    fs::remove_dir_all(&root)?;
    Ok(())
}

fn exercise_table<C: Component>(
    component: C,
    rows: VecModel<String>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut ui = UiDriver::headless(component, VIEWPORT);
    let first_key = rows.row_key(0).ok_or("missing first model key")?;
    assert!(ui.find_row_path(GRID, first_key, None).is_ok());
    assert!(matches!(
        ui.find_row_path(GRID, first_key, Some("missing")),
        Err(TargetError::Missing(_))
    ));
    assert!(matches!(
        ui.find_path("missing"),
        Err(TargetError::Missing(_))
    ));

    for _ in 0..12 {
        scroll(&mut ui);
    }
    assert!(matches!(
        ui.find_row_path(GRID, first_key, None),
        Err(TargetError::Missing(_))
    ));
    let row = update_visible(&mut ui, &rows, 42);
    let key = rows.row_key(row).ok_or("missing updated model key")?;
    let path = ui.find_table_cell_path(GRID, key, 0)?;
    assert_eq!(
        ui.scene().at_path(&path).and_then(|cell| cell.text("text")),
        Some("Updated 042")
    );
    assert!(ui.node(GRID).children.len() < 30);
    assert_eq!(
        ui.node(GRID).list.map(|list| list.row_count),
        Some(ROW_COUNT)
    );

    ui.click_at_path(&path);
    assert_eq!(
        ui.input()
            .focused
            .as_ref()
            .and_then(|target| target.row_key),
        Some(key)
    );
    assert!(
        ui.node(GRID)
            .table_selection
            .as_ref()
            .is_some_and(|selection| selection.contains_cell(key, 0))
    );
    assert!(ui.begin_table_edit());
    ui.fill_focused("Edited by keyboard");
    assert_eq!(ui.finish_table_edit(), TableEditCommit::Changed);
    assert_eq!(rows.row(row).as_deref(), Some("Edited by keyboard"));

    // Keep the model's row key when its source index changes.
    rows.insert(0, "New first row".into());
    ui.update(|_| {});
    assert_eq!(rows.row_index(key), Some(row + 1));
    let path = ui.find_table_cell_path(GRID, key, 0)?;
    assert_eq!(
        ui.scene().at_path(&path).and_then(|cell| cell.text("text")),
        Some("Edited by keyboard")
    );
    Ok(row)
}

#[test]
fn scroll_update_and_edit_the_same_row_on_both_backends() -> Result<(), Box<dyn std::error::Error>>
{
    let (compiled, model) = aot();
    let compiled_row = exercise_table(compiled, model)?;
    let (instance, model) = interpreted()?;
    assert_eq!(exercise_table(instance, model)?, compiled_row);
    Ok(())
}

fn exercise_timers<C: Component>(component: C, counts: impl Fn(&C) -> (i64, i64)) {
    let mut ui = UiDriver::headless(component, VIEWPORT);
    ui.tab();
    assert_eq!(ui.focused(), Some("start"));
    assert!(ui.activate(ActivationKey::Enter));
    assert_eq!(ui.focused(), Some("editor"));
    assert_eq!(counts(ui.component()), (0, 0));
    ui.advance_time(Duration::from_millis(49));
    assert_eq!(counts(ui.component()), (0, 1));
    ui.advance_time(Duration::from_millis(1));
    assert_eq!(counts(ui.component()), (1, 1));
    ui.advance_time(Duration::from_secs(1));
    assert_eq!(counts(ui.component()), (1, 2));
}

#[test]
fn keyboard_and_virtual_timers_work_on_both_backends() -> Result<(), Box<dyn std::error::Error>> {
    let (compiled, _) = aot();
    exercise_timers(compiled, |ui| (ui.get_ticks(), ui.get_heartbeats()));
    let (instance, _) = interpreted()?;
    exercise_timers(instance, |ui| {
        let read = |name| match ui.get(name).unwrap() {
            HostValue::Int(count) => count,
            value => panic!("expected an integer, got {value:?}"),
        };
        (read("ticks"), read("heartbeats"))
    });
    Ok(())
}
