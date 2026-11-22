use crate::{
    can_navigate, folder_name,
    settings::{read_text, write_text},
};
use session::{ConversationActivity, ConversationSummary, Snapshot};
use std::{cell::Cell, io, path::Path, rc::Rc};
use workbench_ui::AgentWorkbench;

const FILE_NAME: &str = "notifications.txt";

#[derive(Clone, Copy, PartialEq, Eq)]
enum CompletionMode {
    Off,
    Background,
    Always,
}

impl CompletionMode {
    fn parse(value: &str) -> io::Result<Self> {
        match value {
            "off" => Ok(Self::Off),
            "background" => Ok(Self::Background),
            "always" => Ok(Self::Always),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "无效的完成通知设置",
            )),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Background => "background",
            Self::Always => "always",
        }
    }

    fn permits(self, window_active: bool) -> bool {
        self == Self::Always || self == Self::Background && !window_active
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Notice {
    Completion,
    Question,
}

fn transitions<'a>(
    snapshot: &'a Snapshot,
    previous: &Snapshot,
    completion: CompletionMode,
    questions: bool,
    window_active: bool,
) -> Vec<(&'a ConversationSummary, Notice)> {
    let was = previous
        .conversations
        .iter()
        .map(|row| (row.id, row.activity))
        .collect::<std::collections::HashMap<_, _>>();
    snapshot
        .conversations
        .iter()
        .filter_map(|row| {
            if row.archived {
                return None;
            }
            let prior = was.get(&row.id).copied();
            if questions
                && row.activity == ConversationActivity::NeedsInput
                && prior != Some(ConversationActivity::NeedsInput)
                && (!window_active || row.id != snapshot.view.conversation)
            {
                Some((row, Notice::Question))
            } else if completion.permits(window_active)
                && matches!(
                    prior,
                    Some(ConversationActivity::Running | ConversationActivity::NeedsInput)
                )
                && matches!(
                    row.activity,
                    ConversationActivity::Idle | ConversationActivity::Updated
                )
            {
                Some((row, Notice::Completion))
            } else {
                None
            }
        })
        .collect()
}

#[derive(Clone, Default)]
pub(super) struct Notifications {
    pending: Rc<Cell<Option<i64>>>,
}

impl Notifications {
    fn open_pending(&self, ui: &mut AgentWorkbench) {
        if can_navigate(ui)
            && let Some(id) = self.pending.take()
            && ui
                .model_conversations()
                .snapshot()
                .iter()
                .any(|row| row.id == id)
        {
            ui.invoke_select_conversation(id);
        }
    }

    pub fn update(
        &self,
        ui: &mut AgentWorkbench,
        snapshot: &Snapshot,
        previous: Option<&Snapshot>,
    ) {
        if ui.get_notifications_authorized()
            && let Some(previous) = previous
            && let Ok(completion) = CompletionMode::parse(&ui.get_completion_notice_mode())
        {
            for (row, notice) in transitions(
                snapshot,
                previous,
                completion,
                ui.get_question_notices(),
                ui.get_notice_window_active(),
            ) {
                let project = folder_name(row.project());
                native::post(row.id, notice, &format!("{project} · {}", row.title));
            }
        }
        self.open_pending(ui);
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
enum NativeEvent {
    Authorization(i64),
    Activation(i64),
    DeliveryFailed,
}

pub(super) fn attach(ui: &mut AgentWorkbench, directory: Option<&Path>) -> Notifications {
    let controller = Notifications::default();
    ui.set_notifications_supported(native::available());
    if let Some(directory) = directory {
        match load(directory) {
            Ok(Some((completion, questions))) => {
                ui.set_completion_notice_mode(completion.key().into());
                ui.set_question_notices(questions);
            }
            Ok(None) => {}
            Err(error) => ui.set_notification_settings_error(format!("无法读取设置（{error}）")),
        }
    }
    let weak = ui.as_weak();
    let directory = directory.map(Path::to_owned);
    ui.on_save_notification_settings(move || {
        let (Some(mut ui), Some(directory)) = (weak.upgrade(), directory.as_deref()) else {
            return;
        };
        let result =
            CompletionMode::parse(&ui.get_completion_notice_mode()).and_then(|completion| {
                let question = if ui.get_question_notices() {
                    "questions"
                } else {
                    "quiet"
                };
                write_text(
                    directory,
                    FILE_NAME,
                    &format!("{} {question}\n", completion.key()),
                )
            });
        ui.set_notification_settings_error(
            result
                .err()
                .map_or_else(String::new, |error| format!("无法保存设置（{error}）")),
        );
    });
    let weak = ui.as_weak();
    ui.on_request_notification_access(move || {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_notifications_supported()
            && ui.get_notification_access_status() == 0
            && !ui.get_notification_access_pending()
        {
            ui.set_notification_access_pending(true);
            native::request();
        }
    });
    ui.on_refresh_notification_access(native::start);

    let weak = ui.as_weak();
    let activation = controller.clone();
    native::bind(move |event| {
        let Some(mut ui) = weak.upgrade() else { return };
        match event {
            NativeEvent::Authorization(status) => {
                ui.set_notification_access_pending(false);
                ui.set_notifications_authorized(status == 1);
                ui.set_notification_access_status(status);
            }
            NativeEvent::Activation(id) => {
                ui.set_notification_settings_open(false);
                ui.invoke_restore_notification_window();
                activation.pending.set(Some(id));
                activation.open_pending(&mut ui);
            }
            NativeEvent::DeliveryFailed => {
                ui.set_notification_settings_error("无法使用系统通知，请检查系统通知设置".into());
                native::start();
            }
        }
    });
    controller
}

fn load(directory: &Path) -> io::Result<Option<(CompletionMode, bool)>> {
    let Some(value) = read_text(directory, FILE_NAME, 64)? else {
        return Ok(None);
    };
    let mut fields = value.split_whitespace();
    match (fields.next(), fields.next(), fields.next()) {
        (Some(mode), Some(question @ ("questions" | "quiet")), None) => Ok(Some((
            CompletionMode::parse(mode)?,
            question == "questions",
        ))),
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "无效的通知设置")),
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::{NativeEvent, Notice};
    use std::{
        cell::RefCell,
        ffi::{CString, c_char},
    };
    use xrack_runtime::invoke_from_event_loop;

    type Route = Box<dyn Fn(NativeEvent)>;
    thread_local! { static ROUTE: RefCell<Option<Route>> = const { RefCell::new(None) }; }

    unsafe extern "C" {
        fn xr_workbench_notifications_available() -> bool;
        fn xr_workbench_notifications_start(callback: extern "C" fn(i32, i64));
        fn xr_workbench_notifications_request();
        fn xr_workbench_notifications_post(
            conversation: i64,
            title: *const c_char,
            body: *const c_char,
            question: bool,
        );
    }

    extern "C" fn receive(event: i32, value: i64) {
        let event = match event {
            0 => NativeEvent::Authorization(value),
            1 => NativeEvent::Activation(value),
            2 => NativeEvent::DeliveryFailed,
            _ => return,
        };
        let _ = invoke_from_event_loop(move || {
            ROUTE.with(|route| {
                if let Some(route) = route.borrow().as_ref() {
                    route(event);
                }
            });
        });
    }

    pub fn bind(route: impl Fn(NativeEvent) + 'static) {
        ROUTE.with(|slot| slot.replace(Some(Box::new(route))));
    }

    pub fn available() -> bool {
        // NSBundle queries the identity of the current process.
        unsafe { xr_workbench_notifications_available() }
    }

    pub fn start() {
        // Called from the workbench UI thread after its event loop is installed.
        unsafe { xr_workbench_notifications_start(receive) }
    }

    pub fn request() {
        // Permission is requested by a user action on the UI thread.
        unsafe { xr_workbench_notifications_request() }
    }

    pub fn post(id: i64, notice: Notice, detail: &str) {
        let question = notice == Notice::Question;
        let title = if question {
            c"需要确认"
        } else {
            c"任务已结束"
        };
        let detail: String = detail
            .chars()
            .filter(|character| !character.is_control())
            .collect();
        if let Ok(body) = CString::new(detail) {
            // The bridge copies both strings before returning.
            unsafe { xr_workbench_notifications_post(id, title.as_ptr(), body.as_ptr(), question) }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    use super::{NativeEvent, Notice};
    pub fn bind(_: impl Fn(NativeEvent) + 'static) {}
    pub fn available() -> bool {
        false
    }
    pub fn start() {}
    pub fn request() {}
    pub fn post(_: i64, _: Notice, _: &str) {}
}

pub(super) fn start() {
    native::start();
}

#[cfg(test)]
mod tests {
    use super::*;
    use workbench_ui::ConversationRow;
    use xrack_runtime::{Component, WindowStatus};

    #[test]
    fn preferences_respect_focus_and_emit_once_for_each_transition() {
        let mut before = Snapshot::connecting("/workspace".into());
        let mut other = before.conversations[0].clone();
        other.id = 1;
        other.activity = ConversationActivity::Running;
        before.conversations[0].activity = ConversationActivity::Running;
        before.conversations.push(other);
        let mut after = before.clone();
        after.conversations[0].activity = ConversationActivity::Idle;
        after.conversations[1].activity = ConversationActivity::NeedsInput;
        let events = |mode, questions, active| {
            transitions(&after, &before, mode, questions, active)
                .into_iter()
                .map(|(row, notice)| (row.id, notice))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            events(CompletionMode::Background, true, true),
            [(1, Notice::Question)]
        );
        assert_eq!(
            events(CompletionMode::Background, false, false),
            [(0, Notice::Completion)]
        );
        assert_eq!(
            events(CompletionMode::Always, true, true),
            [(0, Notice::Completion), (1, Notice::Question)]
        );
        assert!(events(CompletionMode::Off, false, false).is_empty());
        assert!(transitions(&after, &after, CompletionMode::Always, true, true).is_empty());

        after.view.conversation = 1;
        assert!(transitions(&after, &before, CompletionMode::Off, true, true).is_empty());
    }

    #[test]
    fn an_activation_waits_for_navigation_and_opens_the_intended_conversation() {
        let mut ui = AgentWorkbench::new();
        ui.model_conversations().push(ConversationRow {
            id: 8,
            title: "需要继续处理的会话".into(),
            folder: String::new(),
            activity: String::new(),
            shown: true,
            pinned: false,
            pinned_shown: false,
        });
        let calls = Rc::new(Cell::new(-1));
        let output = calls.clone();
        ui.on_select_conversation(move |id| output.set(id));
        let notices = Notifications::default();
        ui.set_switching(true);
        notices.pending.set(Some(8));
        notices.open_pending(&mut ui);
        assert_eq!(calls.get(), -1);
        assert_eq!(notices.pending.get(), Some(8));
        ui.set_switching(false);
        notices.open_pending(&mut ui);
        assert_eq!(calls.get(), 8);
        assert_eq!(notices.pending.get(), None);
    }

    #[test]
    fn notification_preferences_are_restored_without_requesting_access() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let mut initial = AgentWorkbench::new();
        attach(&mut initial, Some(directory.path()));
        assert_eq!(initial.get_completion_notice_mode(), "off");
        assert!(!initial.get_notice_window_active());
        initial.publish_window_status(WindowStatus {
            active: true,
            ..WindowStatus::default()
        });
        assert!(initial.get_notice_window_active());
        initial.set_completion_notice_mode("background".into());
        initial.set_question_notices(true);
        initial.invoke_save_notification_settings();
        let mut next = AgentWorkbench::new();
        attach(&mut next, Some(directory.path()));
        assert_eq!(next.get_completion_notice_mode(), "background");
        assert!(next.get_question_notices());
        assert!(!next.get_notifications_authorized());
        Ok(())
    }
}
