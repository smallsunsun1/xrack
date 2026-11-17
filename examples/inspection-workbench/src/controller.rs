use crate::{InspectionRow, InspectionWorkbench, Project, Record, Status, project::AuditEntry};
use std::{cell::RefCell, collections::HashMap, io, path::PathBuf, rc::Rc};
use xrack::WeakComponent;

pub enum FileJob {
    Open(PathBuf),
    Save {
        path: PathBuf,
        project: Project,
        revision: u64,
    },
}

pub enum FileResult {
    Opened(PathBuf, Project),
    Saved(PathBuf, u64),
}

impl FileJob {
    pub fn run(self) -> io::Result<FileResult> {
        match self {
            Self::Open(path) => {
                let project = Project::read(&path)?;
                Ok(FileResult::Opened(path, project))
            }
            Self::Save {
                path,
                project,
                revision,
            } => {
                project.write(&path)?;
                Ok(FileResult::Saved(path, revision))
            }
        }
    }
}

type FileCompletion = io::Result<FileResult>;
pub type CompleteJob = Box<dyn FnOnce(FileCompletion)>;
type Submit = dyn Fn(FileJob, CompleteJob) -> io::Result<()>;

struct Session {
    project: Project,
    indices: HashMap<i64, usize>,
    scope: Option<Status>,
    path: Option<PathBuf>,
    revision: u64,
    saved_revision: u64,
}

impl Session {
    fn new(project: Project, path: Option<PathBuf>) -> Self {
        let indices = project
            .records
            .iter()
            .enumerate()
            .map(|(index, record)| (record.id, index))
            .collect();
        Self {
            project,
            indices,
            scope: None,
            path,
            revision: 0,
            saved_revision: 0,
        }
    }

    fn publish(&self, ui: &mut InspectionWorkbench) {
        ui.set_project_name(self.project.name.clone());
        ui.set_rows(
            self.project
                .records
                .iter()
                .map(|record| row(record, self.scope))
                .collect(),
        );
        ui.set_scope(-1);
        ui.set_selected_id(-1);
        ui.set_editing_id(-1);
        ui.set_owner_draft(String::new());
        ui.set_state_draft(0);
        ui.set_note_draft(String::new());
        ui.set_unsaved(false);
        self.publish_summary(ui);
    }

    fn publish_summary(&self, ui: &mut InspectionWorkbench) {
        let mut counts = [0; 3];
        for record in &self.project.records {
            counts[record.status.index() as usize] += 1;
        }
        ui.set_total(self.project.records.len() as i64);
        ui.set_pending(counts[0]);
        ui.set_active(counts[1]);
        ui.set_completed(counts[2]);
        ui.set_last_change(
            self.project
                .history
                .last()
                .map_or_else(String::new, |entry| {
                    format!(
                        "最近修改 INSP-{:05} · {}",
                        entry.record,
                        entry.current.label()
                    )
                }),
        );
    }

    fn choose_scope(&mut self, ui: &mut InspectionWorkbench, index: i64) {
        let scope = Status::from_index(index);
        if self.scope == scope {
            return;
        }
        let previous = self.scope;
        self.scope = scope;
        ui.set_scope(index);
        let model = ui.model_rows();
        for (index, record) in self.project.records.iter().enumerate() {
            let visible = scope.is_none_or(|scope| record.status == scope);
            if visible != previous.is_none_or(|scope| record.status == scope)
                && let Some(mut row) = model.row(index)
            {
                row.shown = visible;
                model.set_row(index, row);
            }
        }
    }

    fn show_record(&self, ui: &mut InspectionWorkbench, id: i64) {
        let Some(&index) = self.indices.get(&id) else {
            return;
        };
        let item = &self.project.records[index];
        ui.set_editing_id(id);
        ui.set_editing_label(format!("{} · {}", item.code(), item.asset));
        ui.set_baseline_owner(item.owner.clone());
        ui.set_baseline_state(item.status.index());
        ui.set_baseline_note(item.note.clone());
        ui.set_owner_draft(item.owner.clone());
        ui.set_state_draft(item.status.index());
        ui.set_note_draft(item.note.clone());
    }

    fn apply_draft(&mut self, ui: &mut InspectionWorkbench) {
        if !ui.get_draft_changed() || ui.get_loading() {
            return;
        }
        let id = ui.get_editing_id();
        let Some((&index, status)) = self
            .indices
            .get(&id)
            .zip(Status::from_index(ui.get_state_draft()))
        else {
            ui.set_message("记录或处理状态无效".into());
            return;
        };
        let current = &mut self.project.records[index];
        let next = Record {
            owner: ui.get_owner_draft().trim().to_owned(),
            status,
            note: ui.get_note_draft(),
            ..current.clone()
        };
        if let Err(error) = next.validate() {
            ui.set_message(error.to_string());
            return;
        }
        let change = AuditEntry {
            record: id,
            previous: current.status,
            current: next.status,
            owner: next.owner.clone(),
        };
        ui.model_rows().set_row(index, row(&next, self.scope));
        *current = next;
        self.project.history.push(change);
        self.revision += 1;
        ui.set_unsaved(true);
        self.show_record(ui, id);
        self.publish_summary(ui);
        ui.set_message(format!("已应用 INSP-{id:05} 的修改"));
    }
}

fn row(item: &Record, scope: Option<Status>) -> InspectionRow {
    let code = item.code();
    InspectionRow {
        searchable: format!("{} {} {} {}", code, item.asset, item.site, item.owner),
        id: item.id,
        code,
        asset: item.asset.clone(),
        site: item.site.clone(),
        owner: item.owner.clone(),
        state: item.status.label().into(),
        shown: scope.is_none_or(|scope| scope == item.status),
    }
}

#[derive(Clone)]
struct Controller {
    ui: WeakComponent<InspectionWorkbench>,
    session: Rc<RefCell<Session>>,
    submit: Rc<Submit>,
}

impl Controller {
    fn update(&self, change: impl FnOnce(&mut InspectionWorkbench, &mut Session)) {
        if let Some(mut ui) = self.ui.upgrade() {
            change(&mut ui, &mut self.session.borrow_mut());
        }
    }

    fn open(&self, path: PathBuf) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_unsaved() || ui.get_draft_changed() {
            return;
        }
        self.submit_file(ui, FileJob::Open(path));
    }

    fn save(&self, choice: Option<PathBuf>) {
        let Some(mut ui) = self.ui.upgrade() else {
            return;
        };
        if ui.get_busy() || ui.get_draft_changed() {
            return;
        }
        let job = {
            let session = self.session.borrow();
            choice
                .or_else(|| session.path.clone())
                .map(|path| FileJob::Save {
                    path,
                    project: session.project.clone(),
                    revision: session.revision,
                })
        };
        match job {
            Some(job) => self.submit_file(ui, job),
            None => ui.invoke_show_save_panel(),
        }
    }

    fn submit_file(&self, mut ui: InspectionWorkbench, job: FileJob) {
        if ui.get_busy() {
            return;
        }
        ui.set_busy(true);
        let loading = matches!(job, FileJob::Open(_));
        ui.set_loading(loading);
        ui.set_message(
            if loading {
                "正在读取并校验项目…"
            } else {
                "正在后台写入项目…"
            }
            .into(),
        );
        let controller = self.clone();
        if let Err(error) = (self.submit)(job, Box::new(move |result| controller.complete(result)))
        {
            self.complete(Err(error));
        }
    }

    fn complete(&self, result: FileCompletion) {
        self.update(|ui, session| {
            ui.set_busy(false);
            ui.set_loading(false);
            match result {
                Ok(FileResult::Opened(path, project)) => {
                    *session = Session::new(project, Some(path));
                    session.publish(ui);
                    ui.set_query(String::new());
                    ui.set_message(format!("已打开项目 · {} 条记录", ui.get_total()));
                }
                Ok(FileResult::Saved(path, revision)) => {
                    session.path = Some(path);
                    session.saved_revision = revision;
                    let unsaved = session.revision != session.saved_revision;
                    ui.set_unsaved(unsaved);
                    ui.set_message(
                        if unsaved {
                            "已保存快照，还有较新的修改"
                        } else {
                            "项目已保存"
                        }
                        .into(),
                    );
                }
                Err(error) => ui.set_message(format!("项目操作失败：{error}")),
            }
        });
    }
}

pub fn connect(
    ui: &mut InspectionWorkbench,
    project: Project,
    submit: impl Fn(FileJob, CompleteJob) -> io::Result<()> + 'static,
) {
    let session = Session::new(project, None);
    session.publish(ui);
    let controller = Controller {
        ui: ui.as_weak(),
        session: Rc::new(RefCell::new(session)),
        submit: Rc::new(submit),
    };

    let listener = controller.clone();
    ui.on_choose_scope(move |scope| listener.update(|ui, session| session.choose_scope(ui, scope)));
    let listener = controller.clone();
    ui.on_choose_selection(move |keys| {
        listener.update(|ui, _| {
            ui.set_selected_id(match keys.as_slice() {
                [id] => *id,
                _ => -1,
            })
        })
    });
    let listener = controller.clone();
    ui.on_begin_edit(move || {
        listener.update(|ui, session| session.show_record(ui, ui.get_selected_id()))
    });
    let listener = controller.clone();
    ui.on_discard_edit(move || {
        listener.update(|ui, session| session.show_record(ui, ui.get_editing_id()))
    });
    let listener = controller.clone();
    ui.on_apply_edit(move || listener.update(|ui, session| session.apply_draft(ui)));
    let listener = controller.clone();
    ui.on_read_file(move |path| listener.open(path.into()));
    let listener = controller.clone();
    ui.on_write_file(move |path| listener.save(Some(path.into())));
    ui.on_request_save(move || controller.save(None));
}
