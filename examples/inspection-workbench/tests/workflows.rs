use std::{cell::RefCell, fs, io, path::Path, rc::Rc};
use xrack::{
    Component, FileDialogMode, NativeDialogOptions, NativeDialogQueue, NativeDialogResult,
};
use xrack_inspection_workbench::{
    CompleteJob, FileJob, InspectionWorkbench, Project, Status, connect,
};
use xrack_testing::{Size, UiDriver};

type TestUi = UiDriver<InspectionWorkbench>;
type Pending = Rc<RefCell<Option<(FileJob, CompleteJob)>>>;

fn workbench(
    project: Project,
    submit: impl Fn(FileJob, CompleteJob) -> io::Result<()> + 'static,
) -> TestUi {
    let mut component = InspectionWorkbench::new();
    connect(&mut component, project, submit);
    UiDriver::headless(component, Size::new(1296.0, 760.0))
}

fn immediate(project: Project) -> TestUi {
    workbench(project, |job, finish| {
        finish(job.run());
        Ok(())
    })
}

fn select_and_start(ui: &mut TestUi, index: usize) {
    let model = ui.component().model_rows();
    let key = model.row_key(index).expect("row key");
    let row = model.row(index).expect("selected row");
    let path = ui
        .find_table_cell_path("inspections", key, 0)
        .expect("visible table cell");
    ui.click_at_path(&path);
    assert_eq!(ui.component().get_selected_id(), row.id);
    ui.click("edit-selection");
    assert_eq!(ui.component().get_editing_id(), row.id);
}

fn choose_file(ui: &mut TestUi, mode: FileDialogMode, path: &Path) {
    let scene = ui.component().render_shared();
    let mut queue = NativeDialogQueue::default();
    queue.sync(&scene.native_dialogs.iter().collect::<Vec<_>>());
    let request = queue.begin_next().expect("file dialog");
    assert!(
        matches!(request.options.as_ref(), NativeDialogOptions::File(options) if options.mode == mode)
    );
    ui.update(|_| {
        assert!(queue.complete(
            request.id,
            NativeDialogResult::Files(vec![path.to_str().expect("UTF-8 path").into()]),
        ));
    });
}

#[test]
fn combines_scope_and_search_on_ten_thousand_records() {
    let mut ui = immediate(Project::sample(10_000));
    assert_eq!(ui.component().get_total(), 10_000);
    assert_eq!(ui.component().get_visible_count(), 10_000);
    assert!(ui.node("inspections").children.len() < 30);
    ui.click("active-only");
    assert_eq!(ui.component().get_visible_count(), 2_000);
    ui.click("search");
    ui.fill_focused("INSP-00003");
    assert_eq!(ui.component().get_visible_count(), 1);
    select_and_start(&mut ui, 2);
    ui.click("completed-only");
    assert_eq!(ui.component().get_visible_count(), 0);
    assert_eq!(ui.component().get_editing_id(), 3);
    ui.click("all");
    assert_eq!(ui.component().get_visible_count(), 1);
    assert_eq!(ui.component().get_selected_id(), 3);

    ui.click("search");
    ui.fill_focused("INSP-0000");
    assert_eq!(ui.component().get_visible_count(), 9);
    ui.update(|component| {
        assert!(component.set_table_sort(
            "inspections",
            Some(xrack::TableSort {
                column: 0,
                descending: true
            }),
        ));
    });
    let key = ui.component().model_rows().row_key(8).expect("row nine");
    assert_eq!(
        ui.node("inspections")
            .child_at(0)
            .and_then(|row| row.row_key),
        Some(key)
    );
    assert_eq!(ui.component().get_selected_id(), 3);
    assert_eq!(ui.component().get_editing_id(), 3);
}

#[test]
fn validates_a_draft_preserves_row_identity_and_updates_dashboard() {
    let mut ui = immediate(Project::sample(500));
    let model = ui.component().model_rows();
    let key = model.row_key(0);
    let completed = ui.component().get_completed();
    select_and_start(&mut ui, 0);
    ui.click("state-choice");
    ui.choice_key(xrack::layout::ChoiceKey::End);
    ui.choice_key(xrack::layout::ChoiceKey::Accept);
    assert_eq!(ui.component().get_state_draft(), 2);
    ui.click("apply");
    assert!(ui.component().get_message().contains("请填写复检备注"));
    assert_eq!(ui.component().get_completed(), completed);

    ui.click("owner");
    ui.fill_focused("钱琳");
    ui.click("note");
    ui.fill_focused("复检正常，已释放设备。");
    ui.click("apply");
    assert_eq!(ui.component().get_completed(), completed + 1);
    assert_eq!(model.row_key(0), key);
    assert_eq!(model.row(0).expect("updated row").owner, "钱琳");
    assert!(ui.component().get_unsaved());
    assert!(!ui.component().get_draft_changed());
    assert!(ui.component().get_last_change().contains("INSP-00001"));
}

#[test]
fn saves_snapshots_without_losing_edits_made_during_background_work() -> io::Result<()> {
    let folder = tempfile::tempdir()?;
    let path = folder.path().join("现场巡检.json");
    let pending: Pending = Rc::new(RefCell::new(None));
    let jobs = pending.clone();
    let mut ui = workbench(Project::sample(100), move |job, finish| {
        assert!(jobs.replace(Some((job, finish))).is_none());
        Ok(())
    });

    select_and_start(&mut ui, 0);
    ui.click("owner");
    ui.fill_focused("第一次修改");
    ui.click("apply");
    ui.click("save-project");
    choose_file(&mut ui, FileDialogMode::SaveFile, &path);
    assert!(ui.component().get_busy());
    ui.update(|component| assert!(!component.close_requested()));

    ui.click("owner");
    ui.fill_focused("保存期间修改");
    ui.click("apply");
    let (job, finish) = pending.take().expect("pending save");
    ui.update(|_| finish(job.run()));
    assert!(ui.component().get_unsaved());
    assert_eq!(Project::read(&path)?.records[0].owner, "第一次修改");

    ui.click("save-project");
    let (job, finish) = pending.take().expect("pending second save");
    ui.update(|_| finish(job.run()));
    assert!(!ui.component().get_unsaved());

    let mut reopened = immediate(Project::sample(6));
    reopened.click("open-project");
    choose_file(&mut reopened, FileDialogMode::OpenFile, &path);
    assert_eq!(reopened.component().get_total(), 100);
    assert_eq!(
        reopened
            .component()
            .model_rows()
            .row(0)
            .expect("imported row")
            .owner,
        "保存期间修改"
    );
    assert_eq!(Project::read(&path)?.history.len(), 2);
    Ok(())
}

#[test]
fn rejects_invalid_import_without_replacing_open_project() -> io::Result<()> {
    let folder = tempfile::tempdir()?;
    let path = folder.path().join("bad.json");
    let mut project = Project::sample(3);
    project.records[1].id = project.records[0].id;
    fs::write(&path, serde_json::to_vec(&project)?)?;

    let mut ui = immediate(Project::sample(100));
    ui.click("open-project");
    choose_file(&mut ui, FileDialogMode::OpenFile, &path);
    assert!(ui.component().get_message().contains("重复记录编号"));
    assert_eq!(ui.component().get_total(), 100);
    assert!(!ui.component().get_busy());
    assert!(!ui.component().get_unsaved());

    project.records[1].id = 2;
    project.records[0].status = Status::Completed;
    fs::write(&path, serde_json::to_vec(&project)?)?;
    ui.click("open-project");
    choose_file(&mut ui, FileDialogMode::OpenFile, &path);
    assert!(ui.component().get_message().contains("复检备注"));
    assert_eq!(ui.component().get_total(), 100);
    Ok(())
}

#[test]
fn render_sample_on_gpu() -> Result<(), Box<dyn std::error::Error>> {
    let Some(path) = std::env::var_os("XRACK_WORKBENCH_SNAPSHOT") else {
        return Ok(());
    };
    let mut ui = immediate(Project::sample(10_000));
    select_and_start(&mut ui, 2);
    ui.click("note");
    ui.fill_focused("已重新校准探头，现场读数恢复稳定。\n下次复检时核对温度曲线和告警记录。");
    let mut renderer = pollster::block_on(xrack::renderer::Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let (scene, layout) = renderer.prepare_scene(
        ui.component(),
        ui.component().render_shared(),
        Size::new(1296.0, 760.0),
        ui.input(),
    );
    let frame = renderer.snapshot_prepared(&scene, layout, 2592, 1520, 2.0, ui.input())?;
    frame.write_png(fs::File::create(path)?)?;
    Ok(())
}
