use crate::android::text_traversal::{Traversal, traverse};
use crate::{
    ClipboardShortcut, UserEvent, accessibility::Projection, android::directory::with_activity,
    desktop::WindowEvents,
};
use accesskit::{
    Action, ActionData, ActionRequest, Node, NodeId, Role, Toggled, TreeId, TreeUpdate,
};
use accesskit_winit::WindowEvent;
use android_activity::AndroidApp;
use jni::{
    EnvUnowned, NativeMethod, jni_sig, jni_str,
    objects::{JFloatArray, JIntArray, JObject, JString, JValue},
};
use std::{
    collections::HashMap,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicI64, Ordering},
    },
};
use winit::window::WindowId;
use xrack_runtime::TextCommand;

// The node and action flags mirror XRackAccessibility.java.
const CLICK: i32 = 1;
const FOCUS: i32 = 2;
const SET_TEXT: i32 = 4;
const SET_PROGRESS: i32 = 8;
const SCROLL_UP: i32 = 16;
const SCROLL_DOWN: i32 = 32;
const SCROLL_LEFT: i32 = 64;
const SCROLL_RIGHT: i32 = 128;
const INCREMENT: i32 = 256;
const DECREMENT: i32 = 512;
const SET_SELECTION: i32 = 1024;
const COPY: i32 = 2048;
const CUT: i32 = 4096;
const PASTE: i32 = 8192;
const EXPAND: i32 = 16384;
const COLLAPSE: i32 = 32768;

pub(crate) const CLIPBOARD_ACTIONS: [(i32, TextCommand, &str); 3] = [
    (COPY, TextCommand::Copy, "Copy"),
    (CUT, TextCommand::Cut, "Cut"),
    (PASTE, TextCommand::Paste, "Paste"),
];

pub(crate) fn clipboard_shortcut(id: i32) -> Option<ClipboardShortcut> {
    Some(match id {
        COPY => ClipboardShortcut::Copy,
        CUT => ClipboardShortcut::Cut,
        PASTE => ClipboardShortcut::Paste,
        _ => return None,
    })
}

static NEXT_TOKEN: AtomicI64 = AtomicI64::new(1);
static WINDOWS: LazyLock<Mutex<HashMap<i64, (WindowEvents, WindowId)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) struct Accessibility {
    app: AndroidApp,
    token: Option<i64>,
}

pub(crate) struct SelectionRequest {
    pub token: i64,
    pub node: NodeId,
    pub value: String,
    pub anchor: usize,
    pub focus: usize,
    pub traversal: Option<Traversal>,
}

impl Accessibility {
    pub(crate) fn owns_token(&self, token: i64) -> bool {
        self.token == Some(token)
    }
    pub(crate) fn new(app: AndroidApp, events: WindowEvents, window: WindowId) -> Self {
        let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut windows) = WINDOWS.lock() {
            windows.insert(token, (events, window));
        }
        let mounted = with_activity(&app, |env, activity| {
            let class = env.get_object_class(activity)?;
            // SAFETY: The callbacks have the instance JNI ABI and match XRackActivity.
            let methods = unsafe {
                [
                    NativeMethod::from_raw_parts(
                        jni_str!("accessibilityEnabled"),
                        jni_str!("(JZ)V"),
                        enabled as *mut _,
                    ),
                    NativeMethod::from_raw_parts(
                        jni_str!("accessibilityAction"),
                        jni_str!("(JIILjava/lang/String;F)Z"),
                        perform_action as *mut _,
                    ),
                    NativeMethod::from_raw_parts(
                        jni_str!("accessibilitySetSelection"),
                        jni_str!("(JILjava/lang/String;II)Z"),
                        set_selection as *mut _,
                    ),
                    NativeMethod::from_raw_parts(
                        jni_str!("accessibilityTraverseText"),
                        jni_str!("(JILjava/lang/String;IIIZZ)Z"),
                        traverse_text as *mut _,
                    ),
                ]
            };
            // SAFETY: The native descriptors match the Activity declarations above.
            unsafe { env.register_native_methods(&class, &methods) }?;
            env.call_method(
                activity,
                jni_str!("mountAccessibility"),
                jni_sig!("(J)V"),
                &[JValue::Long(token)],
            )?;
            Ok(())
        });
        let token = if mounted.is_ok() {
            Some(token)
        } else {
            forget(token);
            None
        };
        Self { app, token }
    }

    pub(crate) fn publish_traversal(&self, node: NodeId, traversal: &Traversal) {
        let (Some(token), Ok(node)) = (self.token, i32::try_from(node.0)) else {
            return;
        };
        let _ = with_activity(&self.app, |env, activity| {
            let [start, end] = traversal.segment;
            env.call_method(
                activity,
                jni_str!("publishAccessibilityTraversal"),
                jni_sig!("(JIIIIZ)V"),
                &[
                    JValue::Long(token),
                    JValue::Int(node),
                    JValue::Int(start),
                    JValue::Int(end),
                    JValue::Int(traversal.granularity),
                    JValue::Bool(traversal.forward),
                ],
            )?;
            Ok(())
        });
    }

    pub(crate) fn update_if_active(&mut self, projection: &Projection, update: TreeUpdate) {
        let Some(token) = self.token else { return };
        let Some(tree) = update.tree.as_ref() else {
            return;
        };
        let root = tree.root;
        let nodes: HashMap<_, _> = update.nodes.iter().map(|(id, node)| (*id, node)).collect();
        let Some(root_node) = nodes.get(&root) else {
            return;
        };
        let mut pending: Vec<_> = root_node
            .children()
            .iter()
            .rev()
            .map(|&child| (child, -1))
            .collect();
        let mut snapshots = Vec::new();
        while let Some((id, parent)) = pending.pop() {
            let (Some(&node), Ok(id)) = (nodes.get(&id), i32::try_from(id.0)) else {
                continue;
            };
            if node.is_hidden() || node.role() == Role::TextRun {
                continue;
            }
            snapshots.push((id, parent, node));
            pending.extend(node.children().iter().rev().map(|&child| (child, id)));
        }
        let mut rows = Vec::with_capacity(snapshots.len() * 15);
        let mut ranges = Vec::with_capacity(snapshots.len() * 7);
        let focus = if update.focus == root {
            -1
        } else {
            i32::try_from(update.focus.0).unwrap_or(-1)
        };
        let Ok(string_count) = i32::try_from(4 * snapshots.len()) else {
            return;
        };
        let _ = with_activity(&self.app, |env, activity| {
            let strings =
                env.new_object_array(string_count, jni_str!("java/lang/String"), JObject::null())?;
            for (index, (id, parent, node)) in snapshots.into_iter().enumerate() {
                let [x0, y0, x1, y1] = node.bounds().map_or([0; 4], |rect| {
                    [
                        rect.x0.floor() as i32,
                        rect.y0.floor() as i32,
                        rect.x1.ceil() as i32,
                        rect.y1.ceil() as i32,
                    ]
                });
                rows.extend([id, parent, flags(node), action_bits(node), x0, y0, x1, y1]);
                rows.extend(collection(node));
                rows.extend(
                    projection
                        .published_utf16_selection(NodeId(id as u64), node)
                        .unwrap_or([-1; 2]),
                );
                ranges.extend(
                    [
                        node.min_numeric_value(),
                        node.max_numeric_value(),
                        node.numeric_value(),
                    ]
                    .map(|value| value.unwrap_or(f64::NAN) as f32),
                );
                ranges.extend(
                    [
                        node.scroll_x(),
                        node.scroll_y(),
                        node.scroll_x_max(),
                        node.scroll_y_max(),
                    ]
                    .map(|value| value.unwrap_or(0.0) as f32),
                );
                for (column, text) in [
                    Some(class_name(node)),
                    node.label(),
                    node.value(),
                    node.description().or_else(|| node.placeholder()),
                ]
                .into_iter()
                .enumerate()
                {
                    if let Some(text) = text {
                        env.with_local_frame(2, |env| {
                            let string = env.new_string(text)?;
                            strings.set_element(env, 4 * index + column, &string)
                        })?;
                    }
                }
            }
            let values = JIntArray::new(env, rows.len())?;
            values.set_region(env, 0, &rows)?;
            let range_values = JFloatArray::new(env, ranges.len())?;
            range_values.set_region(env, 0, &ranges)?;
            env.call_method(
                activity,
                jni_str!("updateAccessibility"),
                jni_sig!("(JI[I[F[Ljava/lang/String;)V"),
                &[
                    JValue::Long(token),
                    JValue::Int(focus),
                    JValue::Object(values.as_ref()),
                    JValue::Object(range_values.as_ref()),
                    JValue::Object(strings.as_ref()),
                ],
            )?;
            Ok(())
        });
    }
}

impl Drop for Accessibility {
    fn drop(&mut self) {
        let Some(token) = self.token else { return };
        forget(token);
        let _ = with_activity(&self.app, |env, activity| {
            env.call_method(
                activity,
                jni_str!("unmountAccessibility"),
                jni_sig!("(J)V"),
                &[JValue::Long(token)],
            )?;
            Ok(())
        });
    }
}

fn forget(token: i64) {
    if let Ok(mut windows) = WINDOWS.lock() {
        windows.remove(&token);
    }
}

fn flags(node: &Node) -> i32 {
    i32::from(node.is_disabled())
        | (i32::from(node.toggled().is_some()) << 1)
        | (i32::from(node.toggled() == Some(Toggled::True)) << 2)
        | (i32::from(node.is_selected() == Some(true)) << 3)
        | (i32::from(node.role() == Role::PasswordInput) << 4)
        | (i32::from(node.role() == Role::MultilineTextInput) << 5)
        | (i32::from(node.toggled() == Some(Toggled::Mixed)) << 6)
        | (i32::from(node.role() == Role::Heading) << 7)
        | (i32::from(node.role() == Role::Tree) << 8)
        | (i32::from(node.is_expanded().is_some()) << 9)
        | (i32::from(node.is_expanded() == Some(true)) << 10)
}

fn collection(node: &Node) -> [i32; 5] {
    let integer = |value: Option<usize>| {
        value
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(-1)
    };
    let role = node.role();
    let is_collection = matches!(role, Role::List | Role::ListBox | Role::Tree | Role::Table);
    let row_count = is_collection
        .then(|| node.row_count().or_else(|| node.size_of_set()))
        .flatten();
    let columns = row_count.map(|_| node.column_count().unwrap_or(1));
    let row = match role {
        Role::Cell => node.row_index().filter(|_| node.column_index().is_some()),
        Role::ListItem | Role::ListBoxOption | Role::TreeItem => {
            node.row_index().or_else(|| node.position_in_set())
        }
        _ => None,
    };
    let column = row.map(|_| node.column_index().unwrap_or(0));
    let selection = if node.is_multiselectable() {
        2
    } else if matches!(role, Role::ListBox | Role::Tree | Role::Table)
        || role == Role::List && node.column_count().is_some()
    {
        1
    } else {
        0
    };
    [
        integer(row_count),
        integer(columns),
        integer(row),
        integer(column),
        selection,
    ]
}

fn action_bits(node: &Node) -> i32 {
    if node.is_disabled() {
        return 0;
    }
    let mut bits = 0;
    for (action, bit) in [
        (Action::Click, CLICK),
        (Action::Focus, FOCUS),
        (Action::ScrollUp, SCROLL_UP),
        (Action::ScrollDown, SCROLL_DOWN),
        (Action::ScrollLeft, SCROLL_LEFT),
        (Action::ScrollRight, SCROLL_RIGHT),
        (Action::Increment, INCREMENT),
        (Action::Decrement, DECREMENT),
        (Action::Expand, EXPAND),
        (Action::Collapse, COLLAPSE),
    ] {
        if node.supports_action(action) {
            bits |= bit;
        }
    }
    if node.supports_action(Action::SetValue) && !node.is_read_only() {
        bits |= if node.numeric_value().is_some() {
            SET_PROGRESS
        } else {
            SET_TEXT
        };
    }
    if node.role() != Role::PasswordInput
        && node.value().is_some()
        && node.supports_action(Action::SetTextSelection)
    {
        bits |= SET_SELECTION;
    }
    for action in node.custom_actions() {
        if clipboard_shortcut(action.id).is_some() {
            bits |= action.id;
        }
    }
    bits
}

fn class_name(node: &Node) -> &'static str {
    match node.role() {
        Role::Button | Role::DefaultButton | Role::Tab | Role::MenuItem => "android.widget.Button",
        Role::CheckBox => "android.widget.CheckBox",
        Role::Switch => "android.widget.Switch",
        Role::RadioButton => "android.widget.RadioButton",
        Role::TextInput
        | Role::MultilineTextInput
        | Role::SearchInput
        | Role::PasswordInput
        | Role::SpinButton
        | Role::EditableComboBox => "android.widget.EditText",
        Role::Slider | Role::ScrollBar => "android.widget.SeekBar",
        Role::ProgressIndicator | Role::Meter => "android.widget.ProgressBar",
        Role::List if node.column_count().is_some() => "android.widget.GridView",
        Role::List | Role::ListBox | Role::Tree | Role::Table => "android.widget.ListView",
        Role::ScrollView => "android.widget.ScrollView",
        Role::ComboBox => "android.widget.Spinner",
        Role::Label | Role::Heading | Role::Link => "android.widget.TextView",
        Role::Image => "android.widget.ImageView",
        _ => "android.view.View",
    }
}

fn dispatch(token: i64, event: impl FnOnce(WindowId) -> UserEvent) -> bool {
    let window = WINDOWS
        .lock()
        .ok()
        .and_then(|windows| windows.get(&token).cloned());
    window.is_some_and(|(events, window_id)| events.post(event(window_id)))
}

fn post(token: i64, window_event: WindowEvent) -> bool {
    dispatch(token, |window_id| {
        UserEvent::AndroidAccessibility(
            token,
            accesskit_winit::Event {
                window_id,
                window_event,
            },
        )
    })
}

extern "system" fn set_selection<'local>(
    mut env: EnvUnowned<'local>,
    _: JObject<'local>,
    token: i64,
    node: i32,
    value: JString<'local>,
    anchor: i32,
    focus: i32,
) -> jni::sys::jboolean {
    let (Ok(node), Ok(anchor), Ok(focus)) = (
        u64::try_from(node),
        usize::try_from(anchor),
        usize::try_from(focus),
    ) else {
        return false;
    };
    let result = env.with_env(|env| value.try_to_string(env)).into_outcome();
    let jni::Outcome::Ok(value) = result else {
        return false;
    };
    dispatch(token, |_| {
        UserEvent::AndroidAccessibilitySelection(SelectionRequest {
            token,
            node: NodeId(node),
            value,
            anchor,
            focus,
            traversal: None,
        })
    })
}

extern "system" fn traverse_text<'local>(
    mut env: EnvUnowned<'local>,
    _: JObject<'local>,
    token: i64,
    node: i32,
    value: JString<'local>,
    anchor: i32,
    focus: i32,
    granularity: i32,
    forward: jni::sys::jboolean,
    extend: jni::sys::jboolean,
) -> jni::sys::jboolean {
    let Ok(node) = u64::try_from(node) else {
        return false;
    };
    let previous = match (anchor, focus) {
        (-1, -1) => None,
        (anchor, focus) if anchor >= 0 && focus >= 0 => Some([anchor as usize, focus as usize]),
        _ => return false,
    };
    let result = env.with_env(|env| value.try_to_string(env)).into_outcome();
    let jni::Outcome::Ok(value) = result else {
        return false;
    };
    let Some(traversal) = traverse(&value, previous, granularity, forward, extend) else {
        return false;
    };
    let [anchor, focus] = traversal.selection;
    dispatch(token, |_| {
        UserEvent::AndroidAccessibilitySelection(SelectionRequest {
            token,
            node: NodeId(node),
            value,
            anchor,
            focus,
            traversal: Some(traversal),
        })
    })
}

extern "system" fn enabled<'local>(
    _: EnvUnowned<'local>,
    _: JObject<'local>,
    token: i64,
    active: jni::sys::jboolean,
) {
    post(
        token,
        if !active {
            WindowEvent::AccessibilityDeactivated
        } else {
            WindowEvent::InitialTreeRequested
        },
    );
}

extern "system" fn perform_action<'local>(
    mut env: EnvUnowned<'local>,
    _: JObject<'local>,
    token: i64,
    node: i32,
    code: i32,
    text: JString<'local>,
    progress: f32,
) -> jni::sys::jboolean {
    if node < 0 {
        return false;
    }
    let (action, data) = match code {
        CLICK => (Action::Click, None),
        FOCUS => (Action::Focus, None),
        SCROLL_UP => (Action::ScrollUp, None),
        SCROLL_DOWN => (Action::ScrollDown, None),
        SCROLL_LEFT => (Action::ScrollLeft, None),
        SCROLL_RIGHT => (Action::ScrollRight, None),
        INCREMENT => (Action::Increment, None),
        DECREMENT => (Action::Decrement, None),
        EXPAND => (Action::Expand, None),
        COLLAPSE => (Action::Collapse, None),
        COPY | CUT | PASTE => (Action::CustomAction, Some(ActionData::CustomAction(code))),
        SET_PROGRESS if progress.is_finite() => (
            Action::SetValue,
            Some(ActionData::NumericValue(progress.into())),
        ),
        SET_TEXT => {
            let result = env.with_env(|env| text.try_to_string(env)).into_outcome();
            let jni::Outcome::Ok(text) = result else {
                return false;
            };
            (
                Action::SetValue,
                Some(ActionData::Value(text.into_boxed_str())),
            )
        }
        _ => return false,
    };
    post(
        token,
        WindowEvent::ActionRequested(ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: NodeId(node as u64),
            data,
        }),
    )
}
