use crate::model as ir;
use std::collections::{BTreeMap, BTreeSet};
use xrack_syntax::{
    ast::{self as ast, Diagnostic, ExprKind, Member, Span, Type},
    builtins::{self, FunctionKind, MathFunction},
};

pub use crate::debug_hir::analyze;

type Result<T> = std::result::Result<T, Diagnostic>;

fn fail<T>(span: Span, message: impl Into<String>) -> Result<T> {
    Err(Diagnostic {
        span,
        message: message.into(),
    })
}

pub fn rust_name(name: &str) -> String {
    name.replace('-', "_")
}

pub fn is_reserved_identifier(name: &str) -> bool {
    matches!(name, "_" | "self" | "Self" | "super" | "crate" | "root") || name.starts_with("__")
}

fn check_name(name: &str, span: Span) -> Result<()> {
    if is_reserved_identifier(&rust_name(name)) {
        return fail(span, format!("reserved identifier `{name}`"));
    }
    Ok(())
}

pub fn table_selection_property(kind: &str, name: &str) -> Option<Type> {
    if !table_view(kind) {
        return None;
    }
    match name {
        "current-column" | "selection-count" => Some(Type::Int),
        "has-selection" => Some(Type::Bool),
        _ => None,
    }
}

pub fn table_sort_property(kind: &str, name: &str) -> Option<Type> {
    if !table_view(kind) {
        return None;
    }
    match name {
        "current-sort-column" => Some(Type::Int),
        "current-sort-descending" => Some(Type::Bool),
        _ => None,
    }
}

pub fn table_history_property(kind: &str, name: &str) -> Option<Type> {
    (table_view(kind) && matches!(name, "can-undo" | "can-redo")).then_some(Type::Bool)
}

fn stack_read_type(name: &str) -> Option<Type> {
    Some(match name {
        "current-page" => Type::String,
        "depth" => Type::Int,
        "can-go-back" => Type::Bool,
        _ => return None,
    })
}

fn stack_page_read_type(name: &str) -> Option<Type> {
    match name {
        "active" => Some(Type::Bool),
        "stack-index" => Some(Type::Int),
        _ => None,
    }
}

fn playback_property(name: &str) -> bool {
    matches!(name, "current-frame" | "frame-count")
}

pub fn window_status_property(name: &str) -> bool {
    matches!(
        name,
        "window-active" | "minimized" | "minimized-known" | "maximized" | "full-screen"
    )
}

pub fn viewport_kind(kind: &str) -> bool {
    matches!(
        kind,
        "ScrollView"
            | "Flickable"
            | "ListView"
            | "GridView"
            | "StandardListView"
            | "TreeView"
            | "TableView"
            | "StandardTableView"
    )
}

pub fn content_viewport(kind: &str) -> bool {
    matches!(kind, "ScrollView" | "Flickable")
}

fn flick_motion_property(kind: &str, name: &str) -> Option<Type> {
    if kind != "Flickable" {
        return None;
    }
    match name {
        "dragging" | "flicking" | "moving" | "rebounding" => Some(Type::Bool),
        "horizontal-overshoot" | "vertical-overshoot" => Some(Type::Length),
        _ => None,
    }
}

fn scroll_control(kind: &str) -> bool {
    matches!(kind, "ScrollBar" | "ScrollIndicator")
}

pub fn scale_rotate_property(kind: &str, name: &str) -> Option<Type> {
    if kind != "ScaleRotateGestureHandler" {
        return None;
    }
    Some(match name {
        "active" => Type::Bool,
        "scale" | "rotation" => Type::Float,
        "center-x" | "center-y" => Type::Length,
        _ => return None,
    })
}

pub fn swipe_property(kind: &str, name: &str) -> Option<Type> {
    if kind != "SwipeGestureHandler" {
        return None;
    }
    Some(match name {
        "swiping" => Type::Bool,
        "pressed-x" | "pressed-y" | "current-x" | "current-y" => Type::Length,
        _ => return None,
    })
}

pub fn hover_property(kind: &str, name: &str) -> Option<Type> {
    if kind != "HoverHandler" {
        return None;
    }
    match name {
        "hovered" => Some(Type::Bool),
        "hover-x" | "hover-y" => Some(Type::Length),
        _ => None,
    }
}

pub fn touch_area_property(kind: &str, name: &str) -> Option<Type> {
    if kind != "TouchArea" {
        return None;
    }
    match name {
        "pressed" | "contains-press" | "has-hover" => Some(Type::Bool),
        "active-touch-count" => Some(Type::Int),
        "touch-points" => Some(Type::TouchPoints),
        "mouse-x" | "mouse-y" | "pressed-x" | "pressed-y" => Some(Type::Length),
        _ => None,
    }
}

fn has_scroll_target(members: &[Member]) -> bool {
    members
        .iter()
        .any(|member| matches!(member, Member::Binding(b) if b.name == "target"))
}

fn row_height_view(kind: &str) -> bool {
    matches!(
        kind,
        "ListView" | "StandardListView" | "TableView" | "StandardTableView" | "TreeView"
    )
}

fn row_visibility_view(kind: &str) -> bool {
    table_view(kind)
        || matches!(
            kind,
            "ListView" | "GridView" | "StandardListView" | "TreeView"
        )
}

fn viewport_property(kind: &str, name: &str) -> bool {
    viewport_kind(kind)
        && matches!(
            name,
            "content-x" | "content-y" | "visible-width" | "visible-height"
        )
}

pub fn viewport_offset_property(kind: &str, name: &str) -> bool {
    match name {
        "content-y" => viewport_kind(kind),
        "content-x" => content_viewport(kind) || table_view(kind),
        _ => false,
    }
}

fn box_shadow_type(name: &str) -> Option<Type> {
    Some(match name {
        "drop-shadow-color" | "inner-shadow-color" => Type::Color,
        "drop-shadow-blur"
        | "drop-shadow-offset-x"
        | "drop-shadow-offset-y"
        | "drop-shadow-spread"
        | "inner-shadow-blur"
        | "inner-shadow-offset-x"
        | "inner-shadow-offset-y"
        | "inner-shadow-spread" => Type::Length,
        _ => return None,
    })
}

pub fn control_part_parent(kind: &str) -> Option<&'static str> {
    match kind {
        "ButtonContent" => Some("Button"),
        "SliderHandle" => Some("Slider"),
        _ => None,
    }
}

pub fn builtin_property(kind: &str, name: &str) -> Option<Type> {
    if matches!(kind, "ButtonContent" | "SliderHandle") {
        return builtin_property("Rectangle", name);
    }
    if matches!(kind, "DayCell" | "SplitHandle") {
        return match name {
            "background" | "border-color" | "border-width" | "border-radius" | "opacity"
            | "visible" => builtin_property("Rectangle", name),
            "accessible-description" if kind == "DayCell" => Some(Type::String),
            _ => None,
        };
    }
    if let Some(base) = delegate_base(kind) {
        return if name == "highlighted" {
            Some(Type::Bool)
        } else {
            builtin_property(base, name)
        };
    }
    if !is_builtin(kind) {
        return None;
    }
    if kind == "SystemTrayIcon" {
        return match name {
            "icon" => Some(Type::Image),
            "title" | "tooltip" => Some(Type::String),
            "visible" | "icon-template" => Some(Type::Bool),
            _ => None,
        };
    }
    if let Some(ty) = geometry_property_for(kind, name) {
        return Some(ty);
    }
    let kind = if kind == "ApplicationWindow" {
        if matches!(
            name,
            "padding"
                | "padding-left"
                | "padding-right"
                | "padding-top"
                | "padding-bottom"
                | "spacing"
        ) {
            return Some(Type::Length);
        }
        "Window"
    } else {
        kind
    };
    if viewport_property(kind, name) {
        return Some(Type::Length);
    }
    if kind == "Window" && window_status_property(name) {
        return Some(Type::Bool);
    }
    if name == "mouse-drag-pan-enabled" && viewport_kind(kind) {
        return Some(Type::Bool);
    }
    if name == "preserve-scroll-anchor" && viewport_kind(kind) && !content_viewport(kind) {
        return Some(Type::Bool);
    }
    if name.starts_with("transform-")
        && matches!(
            kind,
            "Window"
                | "Dialog"
                | "Drawer"
                | "PopupWindow"
                | "SecondaryWindow"
                | "TimePickerPopup"
                | "DatePickerPopup"
                | "ColorPickerPopup"
                | "FontPickerPopup"
                | "Menu"
                | "MenuItem"
                | "MenuSeparator"
                | "Tooltip"
                | "Tab"
                | "TabButton"
                | "TableColumn"
        )
    {
        return None;
    }
    if name == "clip" {
        return (matches!(kind, "Rectangle" | "Flickable") || pane(kind)).then_some(Type::Bool);
    }
    if let Some(ty) = box_shadow_type(name) {
        return (kind == "Rectangle").then_some(ty);
    }
    if kind == "SecondaryWindow" {
        return (name == "visible").then_some(Type::Bool);
    }
    if pane(kind) {
        match name {
            "background" | "border-color" => return Some(Type::Brush),
            "content-width"
            | "content-height"
            | "padding"
            | "padding-left"
            | "padding-right"
            | "padding-top"
            | "padding-bottom"
            | "border-width"
            | "border-radius"
            | "border-top-left-radius"
            | "border-top-right-radius"
            | "border-bottom-left-radius"
            | "border-bottom-right-radius" => return Some(Type::Length),
            "enabled" => return Some(Type::Bool),
            _ => {}
        }
    }
    if kind == "ToolSeparator" {
        match name {
            "orientation" => return Some(Type::Orientation),
            "stroke-width" | "padding" => return Some(Type::Length),
            "color" => return Some(Type::Color),
            _ => {}
        }
    }
    if kind == "RoundButton" && name == "radius" {
        return Some(Type::Length);
    }
    if kind == "ButtonGroup" {
        return match name {
            "accessible-name" | "current-value" => Some(Type::String),
            "enabled" | "exclusive" => Some(Type::Bool),
            "current-index" => Some(Type::Int),
            "check-state" => Some(Type::CheckState),
            _ => None,
        };
    }
    if kind == "Action" {
        return match name {
            "text" | "shortcut" => Some(Type::String),
            "icon" => Some(Type::Image),
            "enabled" | "checkable" | "checked" => Some(Type::Bool),
            _ => None,
        };
    }
    if kind == "ActionGroup" {
        return match name {
            "enabled" | "exclusive" => Some(Type::Bool),
            "current-value" => Some(Type::String),
            _ => None,
        };
    }
    if kind == "Timer" {
        return match name {
            "interval" => Some(Type::Int),
            "running" | "repeat" | "triggered-on-start" => Some(Type::Bool),
            _ => None,
        };
    }
    if kind == "FileDialog" {
        return match name {
            "mode" => Some(Type::FileDialogMode),
            "title" | "directory" | "file-name" => Some(Type::String),
            "name-filters" => Some(Type::StringList),
            "enabled" => Some(Type::Bool),
            _ => None,
        };
    }
    if kind == "MessageDialog" {
        return match name {
            "level" => Some(Type::MessageLevel),
            "buttons" => Some(Type::MessageButtons),
            "title" | "text" | "informative-text" => Some(Type::String),
            "enabled" => Some(Type::Bool),
            _ => None,
        };
    }
    if kind == "Shortcut" {
        return match name {
            "sequence" => Some(Type::String),
            "sequences" => Some(Type::StringList),
            "enabled" | "visible" | "auto-repeat" | "focus-within" => Some(Type::Bool),
            _ => None,
        };
    }
    if matches!(name, "z" | "opacity")
        && matches!(kind, "MenuItem" | "MenuSeparator" | "TableColumn")
    {
        return None;
    }
    if let Some(ty) = layout_item_property(name) {
        return Some(ty);
    }
    if kind == "StandardButton" {
        return match name {
            "kind" => Some(Type::StandardButtonKind),
            "primary" => Some(Type::Bool),
            _ => builtin_property("Button", name),
        };
    }
    if kind == "MediaPlayer" {
        match name {
            "source" | "subtitle-file" => return Some(Type::String),
            "autoplay" | "muted" => return Some(Type::Bool),
            "volume" | "playback-rate" => return Some(Type::Float),
            "loops" | "audio-track" | "subtitle-track" | "network-bandwidth" => {
                return Some(Type::Int);
            }
            "image-fit" => return Some(Type::ImageFit),
            _ => {
                if let Some(ty) = media_read_type(name) {
                    return Some(ty);
                }
            }
        }
    }
    if kind == "AnimatedImage" {
        return match name {
            "playing" | "paused" => Some(Type::Bool),
            "speed" => Some(Type::Float),
            "loops" => Some(Type::Int),
            name if playback_property(name) => Some(Type::Int),
            _ => builtin_property("Image", name),
        };
    }
    if kind == "Image" {
        return match name {
            "source" => Some(Type::Image),
            "image-fit" => Some(Type::ImageFit),
            "colorize" => Some(Type::Brush),
            "source-clip-x" | "source-clip-y" | "source-clip-width" | "source-clip-height" => {
                Some(Type::Int)
            }
            "horizontal-tile-mode" | "vertical-tile-mode" => Some(Type::ImageTileMode),
            "horizontal-alignment" => Some(Type::ImageHorizontalAlignment),
            "vertical-alignment" => Some(Type::ImageVerticalAlignment),
            "image-rendering" => Some(Type::ImageRendering),
            "accessible-name" | "accessible-description" => Some(Type::String),
            "visible" => Some(Type::Bool),
            _ => None,
        };
    }
    if kind == "StyledText" {
        return match name {
            "text" => Some(Type::StyledText),
            "link-color" => Some(Type::Color),
            "enabled" => Some(Type::Bool),
            _ => builtin_property("Text", name),
        };
    }
    if kind == "Label" {
        return builtin_property("Text", name);
    }
    if focus_property(kind, name) {
        return Some(Type::Bool);
    }
    if let Some(ty) = table_selection_property(kind, name) {
        return Some(ty);
    }
    if let Some(ty) = table_sort_property(kind, name) {
        return Some(ty);
    }
    if let Some(ty) = table_history_property(kind, name) {
        return Some(ty);
    }
    if let Some(ty) = hover_property(kind, name).or_else(|| touch_area_property(kind, name)) {
        return Some(ty);
    }
    if kind == "StackView"
        && let Some(ty) = stack_read_type(name)
    {
        return Some(ty);
    }
    if kind == "StackPage"
        && let Some(ty) = stack_page_read_type(name)
    {
        return Some(ty);
    }
    Some(match name {
        "accessible-name" | "accessible-description" => Type::String,
        "value" if kind == "RadioButton" => Type::String,
        "group" if matches!(kind, "RadioButton" | "CheckBox" | "Switch") => Type::String,
        "color-scheme" if kind == "Window" => Type::ColorScheme,
        "accent-color" if kind == "Window" => Type::Color,
        "no-frame" | "resizable" if kind == "Window" => Type::Bool,
        "resize-border-width" if kind == "Window" => Type::Length,
        "initial-focus" if kind == "FocusScope" => Type::String,
        "focus-on-click" if matches!(kind, "Button" | "ToolButton" | "RoundButton") => Type::Bool,
        "focus-on-click" | "focus-on-tab-navigation" | "restore-focus" | "enabled"
            if kind == "FocusScope" =>
        {
            Type::Bool
        }
        "enabled"
            if matches!(
                kind,
                "DragArea"
                    | "DropArea"
                    | "WindowMoveArea"
                    | "ScaleRotateGestureHandler"
                    | "SwipeGestureHandler"
                    | "WheelHandler"
                    | "HoverHandler"
            ) =>
        {
            Type::Bool
        }
        "blocking" if kind == "HoverHandler" => Type::Bool,
        "cursor-shape" if matches!(kind, "HoverHandler" | "TouchArea") => Type::CursorShape,
        "handle-swipe-left" | "handle-swipe-right" | "handle-swipe-up" | "handle-swipe-down"
            if kind == "SwipeGestureHandler" =>
        {
            Type::Bool
        }
        "data" | "drag-key" if kind == "DragArea" => Type::String,
        "allow-copy" | "allow-move" | "allow-link" if kind == "DragArea" => Type::Bool,
        "keys" if kind == "DropArea" => Type::StringList,
        "accept-files" if kind == "DropArea" => Type::Bool,
        "file-extensions" if kind == "DropArea" => Type::StringList,
        "drop-action" if kind == "DropArea" => Type::DragAction,
        "default-font" if kind == "Window" => Type::Font,
        "default-font-family" if kind == "Window" => Type::String,
        "default-font-size" if kind == "Window" => Type::Length,
        "default-font-weight" if kind == "Window" => Type::Int,
        "default-font-italic" if kind == "Window" => Type::Bool,
        "background"
            if matches!(
                kind,
                "Window"
                    | "Rectangle"
                    | "FocusScope"
                    | "Page"
                    | "PageHeader"
                    | "PageFooter"
                    | "SwipeContent"
                    | "SwipeLeft"
                    | "SwipeRight"
                    | "Button"
                    | "ToolButton"
                    | "RoundButton"
                    | "DelayButton"
                    | "DialogButton"
                    | "GroupBox"
                    | "ToolBar"
                    | "MenuBar"
                    | "TabBar"
                    | "Tumbler"
                    | "Calendar"
                    | "TimePicker"
                    | "TimePickerPopup"
                    | "DatePickerPopup"
                    | "ColorPickerPopup"
                    | "FontPickerPopup"
                    | "Dialog"
                    | "Drawer"
                    | "PopupWindow"
                    | "ListView"
                    | "GridView"
                    | "StandardListView"
                    | "TableView"
                    | "StandardTableView"
                    | "TableCell"
                    | "TreeCell"
                    | "SplitView"
                    | "Tooltip"
                    | "TreeView"
            ) || text_input(kind) =>
        {
            Type::Brush
        }
        "title"
            if matches!(
                kind,
                "Window"
                    | "Page"
                    | "Tab"
                    | "RadioGroup"
                    | "Dialog"
                    | "Drawer"
                    | "Menu"
                    | "TableColumn"
                    | "GroupBox"
                    | "TimePickerPopup"
                    | "DatePickerPopup"
                    | "ColorPickerPopup"
                    | "FontPickerPopup"
            ) =>
        {
            Type::String
        }
        "text"
            if matches!(
                kind,
                "Text"
                    | "Button"
                    | "ToolButton"
                    | "RoundButton"
                    | "DelayButton"
                    | "DialogButton"
                    | "MenuItem"
                    | "TabButton"
                    | "TableCell"
                    | "TreeCell"
                    | "Tooltip"
            ) || toggle(kind)
                || text_input(kind)
                || kind == "RadioButton" =>
        {
            Type::String
        }
        "color" if kind == "ColorPickerPopup" => Type::Color,
        "color"
            if matches!(
                kind,
                "Text"
                    | "Button"
                    | "ToolButton"
                    | "RoundButton"
                    | "DelayButton"
                    | "DialogButton"
                    | "GroupBox"
                    | "Dialog"
                    | "Drawer"
                    | "TableCell"
                    | "TreeCell"
                    | "Tooltip"
            ) || toggle(kind)
                || text_input(kind)
                || matches!(kind, "SpinBox" | "DoubleSpinBox" | "ComboBox")
                || matches!(
                    kind,
                    "RadioButton"
                        | "PageIndicator"
                        | "Tumbler"
                        | "ProgressIndicator"
                        | "BusyIndicator"
                        | "Spinner"
                ) =>
        {
            Type::Color
        }
        "current-color" if kind == "PageIndicator" => Type::Color,
        "font" if has_font(kind) => Type::Font,
        "font-size" if has_font(kind) => Type::Length,
        "font-family" if has_font(kind) => Type::String,
        "font-weight" if has_font(kind) => Type::Int,
        "font-italic" if has_font(kind) => Type::Bool,
        "enabled"
            if matches!(
                kind,
                "Button"
                    | "ToolButton"
                    | "RoundButton"
                    | "DelayButton"
                    | "DialogButton"
                    | "GroupBox"
                    | "Dialog"
                    | "Drawer"
                    | "DialogButtonBox"
                    | "PopupWindow"
                    | "TouchArea"
                    | "Slider"
                    | "Dial"
                    | "RangeSlider"
                    | "SpinBox"
                    | "DoubleSpinBox"
                    | "ComboBox"
                    | "RadioGroup"
                    | "RadioButton"
                    | "TabWidget"
                    | "Tab"
                    | "TabBar"
                    | "TabButton"
                    | "SwipeView"
                    | "SwipeDelegate"
                    | "PageIndicator"
                    | "Calendar"
                    | "TimePicker"
                    | "TimePickerPopup"
                    | "DatePickerPopup"
                    | "ColorPickerPopup"
                    | "FontPickerPopup"
                    | "Tumbler"
                    | "ProgressIndicator"
                    | "BusyIndicator"
                    | "Spinner"
                    | "SwipePage"
                    | "StackView"
                    | "StackPage"
                    | "Page"
                    | "PageHeader"
                    | "PageFooter"
                    | "ScrollView"
                    | "Flickable"
                    | "ListView"
                    | "GridView"
                    | "StandardListView"
                    | "TableView"
                    | "StandardTableView"
                    | "TableCell"
                    | "TreeCell"
                    | "MenuBar"
                    | "ToolBar"
                    | "Menu"
                    | "MenuItem"
                    | "ContextMenuArea"
                    | "SplitView"
                    | "TreeView"
            ) || toggle(kind)
                || text_input(kind) =>
        {
            Type::Bool
        }
        "checked" if toggle(kind) || kind == "DelayButton" => Type::Bool,
        "check-state" if kind == "CheckBox" => Type::CheckState,
        "tristate" if kind == "CheckBox" => Type::Bool,
        "checked" | "checkable" if matches!(kind, "MenuItem" | "GroupBox") => Type::Bool,
        "content-padding" if kind == "GroupBox" => Type::Length,
        "shortcut" if kind == "MenuItem" => Type::String,
        "open" | "close-on-escape" | "close-on-outside"
            if matches!(kind, "Dialog" | "Drawer" | "PopupWindow") || picker_popup(kind) =>
        {
            Type::Bool
        }
        "edge" if kind == "Drawer" => Type::DrawerEdge,
        "drag-margin" if kind == "Drawer" => Type::Length,
        "placement" if kind == "PopupWindow" => Type::PopupPlacement,
        "anchor-gap" if kind == "PopupWindow" => Type::Length,
        "modal" | "focus" | "dim" if kind == "PopupWindow" => Type::Bool,
        "role" if kind == "DialogButton" => Type::DialogRole,
        "primary" if kind == "DialogButton" => Type::Bool,
        "value" | "minimum" | "maximum" | "step" if matches!(kind, "Slider" | "Dial") => {
            Type::Float
        }
        "start-angle" | "end-angle" if kind == "Dial" => Type::Float,
        "input-mode" if kind == "Dial" => Type::DialInputMode,
        "wrap" if kind == "Dial" => Type::Bool,
        "lower-value" | "upper-value" | "minimum" | "maximum" | "step" if kind == "RangeSlider" => {
            Type::Float
        }
        "orientation"
            if matches!(
                kind,
                "Slider"
                    | "RangeSlider"
                    | "ScrollBar"
                    | "ScrollIndicator"
                    | "RadioGroup"
                    | "TabWidget"
                    | "TabBar"
                    | "SwipeView"
                    | "WheelHandler"
                    | "PageIndicator"
                    | "SplitView"
                    | "FlowLayout"
                    | "ToolBar"
            ) =>
        {
            Type::Orientation
        }
        "handle-size" | "handle-hit-size" if kind == "SplitView" => Type::Length,
        "resizing" if kind == "SplitView" => Type::Bool,
        "handle-color" if kind == "SplitView" => Type::Color,
        "delay" if matches!(kind, "Tooltip" | "DelayButton") => Type::Int,
        "press-and-hold-interval" if kind == "TouchArea" => Type::Int,
        "accepted-buttons" if kind == "TouchArea" => Type::PointerButtons,
        "multi-touch" if kind == "TouchArea" => Type::Bool,
        "timeout" if kind == "Tooltip" => Type::Int,
        "progress-color" if kind == "DelayButton" => Type::Color,
        "value" | "minimum" | "maximum" | "step-size" if kind == "SpinBox" => Type::Int,
        "editable"
            if matches!(
                kind,
                "SpinBox" | "DoubleSpinBox" | "ComboBox" | "TableColumn"
            ) =>
        {
            Type::Bool
        }
        "value" | "minimum" | "maximum" | "step-size" if kind == "DoubleSpinBox" => Type::Float,
        "decimals" if kind == "DoubleSpinBox" => Type::Int,
        "decimal-separator" if matches!(kind, "DoubleSpinBox" | "TextInput" | "LineEdit") => {
            Type::String
        }
        "wrap" if matches!(kind, "SpinBox" | "DoubleSpinBox" | "Tumbler") => Type::Bool,
        "wrap" if matches!(kind, "Text" | "TextEdit" | "TextArea") => Type::TextWrap,
        "horizontal-alignment" if kind == "Text" => Type::TextHorizontalAlignment,
        "vertical-alignment" if kind == "Text" => Type::TextVerticalAlignment,
        "overflow" if kind == "Text" => Type::TextOverflow,
        "max-lines" if kind == "Text" => Type::Int,
        "input-type" if matches!(kind, "TextInput" | "LineEdit") => Type::InputType,
        "password-visible" if matches!(kind, "TextInput" | "LineEdit") => Type::Bool,
        "maximum-length" if matches!(kind, "TextInput" | "LineEdit" | "TextEdit" | "TextArea") => {
            Type::Int
        }
        "model" if matches!(kind, "ComboBox" | "Tumbler" | "StandardListView") => Type::StringList,
        "text-role" if matches!(kind, "StandardListView" | "ComboBox") => Type::String,
        "value-role" | "enabled-role" if kind == "ComboBox" => Type::String,
        "auto-sort" if kind == "StandardTableView" => Type::Bool,
        "model" if kind == "StandardTableView" => Type::RecordList("Record".into()),
        "field" if kind == "TableColumn" => Type::String,
        "follow-current"
            if matches!(kind, "StandardListView" | "GridView" | "StandardTableView") =>
        {
            Type::Bool
        }
        "cell-width" | "cell-height" | "spacing" if kind == "GridView" => Type::Length,
        "suggestions" if kind == "SearchField" => Type::StringList,
        "suggestions-for" if kind == "SearchField" => Type::String,
        "live" if matches!(kind, "SearchField" | "Slider" | "RangeSlider") => Type::Bool,
        "wheel-enabled"
            if matches!(
                kind,
                "Slider" | "RangeSlider" | "Dial" | "SpinBox" | "DoubleSpinBox"
            ) =>
        {
            Type::Bool
        }
        "current-index"
            if matches!(
                kind,
                "ComboBox"
                    | "RadioGroup"
                    | "TabWidget"
                    | "TabBar"
                    | "SwipeView"
                    | "ListView"
                    | "GridView"
                    | "StandardListView"
            ) =>
        {
            Type::Int
        }
        "current-index" | "count" if matches!(kind, "PageIndicator" | "Tumbler") => Type::Int,
        "visible-item-count" if kind == "Tumbler" => Type::Int,
        "date" | "minimum-date" | "maximum-date"
            if matches!(kind, "Calendar" | "MonthGrid" | "DatePickerPopup") =>
        {
            Type::Date
        }
        "display-month" if matches!(kind, "Calendar" | "MonthGrid" | "WeekNumberColumn") => {
            Type::Date
        }
        "today" if kind == "MonthGrid" => Type::Date,
        "week-start"
            if matches!(
                kind,
                "Calendar" | "MonthGrid" | "DayOfWeekRow" | "WeekNumberColumn" | "DatePickerPopup"
            ) =>
        {
            Type::Int
        }
        "weekday-labels" if matches!(kind, "Calendar" | "DayOfWeekRow" | "DatePickerPopup") => {
            Type::StringList
        }
        "time" if time_picker(kind) => Type::Time,
        "use-24-hour-format" | "show-seconds" if time_picker(kind) => Type::Bool,
        "accept-text" | "cancel-text" if picker_popup(kind) => Type::String,
        "show-alpha" | "show-color-plane" if kind == "ColorPickerPopup" => Type::Bool,
        "font" if kind == "FontPickerPopup" => Type::Font,
        "preview-text" if kind == "FontPickerPopup" => Type::String,
        "interactive" if matches!(kind, "SwipeView" | "PageIndicator") => Type::Bool,
        "open-side" if kind == "SwipeDelegate" => Type::SwipeSide,
        "initial-page" if kind == "StackView" => Type::String,
        "transition-duration" if kind == "StackView" => Type::Int,
        "dot-size" | "spacing" if kind == "PageIndicator" => Type::Length,
        "current-row" | "frozen-columns" if table_view(kind) => Type::Int,
        "current-key" | "root-key" if kind == "TreeView" => Type::Int,
        "filter-field" | "filter-text" if matches!(kind, "TreeView" | "StandardTableView") => {
            Type::String
        }
        "key" | "parent-key" if kind == "TreeRow" => Type::Int,
        "indentation" if kind == "TreeView" => Type::Length,
        "row-height"
            if matches!(kind, "ListView" | "StandardListView" | "TreeView") || table_view(kind) =>
        {
            Type::Length
        }
        "row-height-field" if row_height_view(kind) => Type::String,
        "row-visible-field" if row_visibility_view(kind) => Type::String,
        "header-height" | "row-header-width" if table_view(kind) => Type::Length,
        "row-header-text" if kind == "TableRow" => Type::String,
        "row-header-field" if kind == "StandardTableView" => Type::String,
        "resizable-columns" | "resizable-rows" | "movable-columns" | "movable-rows"
        | "copyable"
            if table_view(kind) =>
        {
            Type::Bool
        }
        "pasteable" | "undoable" if table_view(kind) => Type::Bool,
        "edit-on-click"
        | "edit-on-selected-click"
        | "edit-on-double-click"
        | "edit-on-key"
        | "edit-on-tab"
        | "edit-on-typing"
            if table_view(kind) =>
        {
            Type::Bool
        }
        "multi-select" if table_view(kind) || kind == "TreeView" => Type::Bool,
        "drag-select" if table_view(kind) => Type::Bool,
        "selection-target" if table_view(kind) => Type::SelectionTarget,
        "sortable" | "resizable" if kind == "TableColumn" => Type::Bool,
        "current-value" if matches!(kind, "ComboBox" | "RadioGroup") => Type::String,
        "progress" if matches!(kind, "ProgressIndicator" | "Spinner") => Type::Float,
        "indeterminate" if matches!(kind, "ProgressIndicator" | "Spinner") => Type::Bool,
        "running" if kind == "BusyIndicator" => Type::Bool,
        "animated" if matches!(kind, "ProgressIndicator" | "Spinner" | "BusyIndicator") => {
            Type::Bool
        }
        "track-color" if matches!(kind, "ProgressIndicator" | "Spinner" | "BusyIndicator") => {
            Type::Color
        }
        "edit-text" if kind == "ComboBox" => Type::String,
        "icon"
            if matches!(
                kind,
                "Button" | "ToolButton" | "RoundButton" | "DelayButton" | "MenuItem"
            ) =>
        {
            Type::Image
        }
        "icon-size"
            if matches!(
                kind,
                "Button" | "ToolButton" | "RoundButton" | "DelayButton"
            ) =>
        {
            Type::Length
        }
        "icon-only"
            if matches!(
                kind,
                "Button" | "ToolButton" | "RoundButton" | "DelayButton"
            ) =>
        {
            Type::Bool
        }
        "wrap" if matches!(kind, "ToolBar" | "DialogButtonBox") => Type::Bool,
        "auto-complete" if kind == "ComboBox" => Type::Bool,
        "placeholder-text" if text_input(kind) || kind == "ComboBox" => Type::String,
        "context-menu-enabled"
            if text_input(kind)
                || matches!(
                    kind,
                    "ComboBox" | "SpinBox" | "DoubleSpinBox" | "TableCell" | "TreeCell"
                ) =>
        {
            Type::Bool
        }
        "read-only"
            if text_input(kind)
                || matches!(kind, "SpinBox" | "DoubleSpinBox" | "TableCell" | "TreeCell") =>
        {
            Type::Bool
        }
        "padding" if matches!(kind, "TableCell" | "TreeCell" | "Tooltip") => Type::Length,
        "content-width" | "content-height" if content_viewport(kind) => Type::Length,
        "viewport-height" if kind == "ScrollView" => Type::Length,
        "vertical-scroll-bar-policy"
            if matches!(
                kind,
                "ScrollView"
                    | "ListView"
                    | "GridView"
                    | "StandardListView"
                    | "TableView"
                    | "StandardTableView"
                    | "TreeView"
                    | "TabBar"
            ) =>
        {
            Type::ScrollBarPolicy
        }
        "horizontal-scroll-bar-policy"
            if table_view(kind) || matches!(kind, "ScrollView" | "TabBar") =>
        {
            Type::ScrollBarPolicy
        }
        "policy" if scroll_control(kind) => Type::ScrollBarPolicy,
        "position" | "size" | "minimum-size" if scroll_control(kind) => Type::Float,
        "step-size" if kind == "ScrollBar" => Type::Float,
        "snap-mode" if matches!(kind, "ScrollBar" | "Slider" | "RangeSlider") => {
            Type::ScrollSnapMode
        }
        "interactive" if matches!(kind, "ScrollBar" | "Flickable") => Type::Bool,
        "flick-direction" if kind == "Flickable" => Type::FlickDirection,
        "bounds-behavior" if kind == "Flickable" => Type::FlickBoundsBehavior,
        "bounds-movement" if kind == "Flickable" => Type::FlickBoundsMovement,
        "rebound-duration" if kind == "Flickable" => Type::Int,
        "spacing" | "padding" | "padding-left" | "padding-right" | "padding-top"
        | "padding-bottom"
            if linear_layout(kind)
                || matches!(
                    kind,
                    "GridLayout" | "RadioGroup" | "Dialog" | "Drawer" | "Page"
                ) =>
        {
            Type::Length
        }
        "spacing" if kind == "TabBar" => Type::Length,
        "horizontal-spacing" | "vertical-spacing"
            if matches!(kind, "GridLayout" | "FlowLayout" | "ToolBar") =>
        {
            Type::Length
        }
        "padding"
            if matches!(
                kind,
                "Button" | "ToolButton" | "RoundButton" | "DialogButton" | "DelayButton"
            ) || text_input(kind) =>
        {
            Type::Length
        }
        "alignment" if linear_layout(kind) => Type::LayoutAlignment,
        "cross-axis-alignment" if linear_layout(kind) => Type::CrossAxisAlignment,
        "flex-direction" if kind == "FlexboxLayout" => Type::FlexboxLayoutDirection,
        "flex-wrap" if kind == "FlexboxLayout" => Type::FlexboxLayoutWrap,
        "cross-axis-line-alignment" if kind == "FlexboxLayout" => Type::LayoutAlignment,
        "spacing-horizontal" | "spacing-vertical" if kind == "FlexboxLayout" => Type::Length,
        "border-radius" | "border-width"
            if matches!(
                kind,
                "Rectangle"
                    | "Button"
                    | "ToolButton"
                    | "RoundButton"
                    | "DialogButton"
                    | "Dialog"
                    | "Drawer"
            ) || text_input(kind) =>
        {
            Type::Length
        }
        "border-top-left-radius"
        | "border-top-right-radius"
        | "border-bottom-right-radius"
        | "border-bottom-left-radius"
            if matches!(
                kind,
                "Rectangle"
                    | "Button"
                    | "ToolButton"
                    | "RoundButton"
                    | "DialogButton"
                    | "Dialog"
                    | "Drawer"
            ) || text_input(kind) =>
        {
            Type::Length
        }
        "border-color"
            if matches!(
                kind,
                "Rectangle"
                    | "Button"
                    | "ToolButton"
                    | "RoundButton"
                    | "DialogButton"
                    | "Dialog"
                    | "Drawer"
            ) || text_input(kind) =>
        {
            Type::Brush
        }
        "commands" if kind == "Path" => Type::String,
        "fill" | "stroke" if kind == "Path" => Type::Brush,
        "stroke-width" if matches!(kind, "Path" | "Spinner" | "BusyIndicator") => Type::Length,
        "fit" if kind == "Path" => Type::ImageFit,
        "viewbox-x" | "viewbox-y" | "viewbox-width" | "viewbox-height" if kind == "Path" => {
            Type::Float
        }
        _ => return None,
    })
}

pub fn geometry_property(name: &str) -> Option<Type> {
    Some(match name {
        "layout-x"
        | "layout-y"
        | "layout-width"
        | "layout-height"
        | "window-bounds-x"
        | "window-bounds-y"
        | "window-bounds-width"
        | "window-bounds-height"
        | "available-width"
        | "available-height" => Type::Length,
        "layout-ready" => Type::Bool,
        _ => return None,
    })
}

fn pane(kind: &str) -> bool {
    matches!(kind, "Pane" | "ControlFrame")
}

pub fn is_window(kind: &str) -> bool {
    matches!(kind, "Window" | "ApplicationWindow")
}

fn content_geometry_owner(kind: &str) -> bool {
    pane(kind) || kind == "ApplicationWindow"
}

fn geometry_property_for(kind: &str, name: &str) -> Option<Type> {
    if !geometry_owner(kind)
        || matches!(name, "available-width" | "available-height") && !content_geometry_owner(kind)
    {
        return None;
    }
    geometry_property(name)
}

pub fn geometry_owner(kind: &str) -> bool {
    !nonvisual_element(kind) && !matches!(kind, "TableColumn" | "SystemTrayIcon")
}

pub fn layout_item_property(name: &str) -> Option<Type> {
    Some(match name {
        "width" | "height" | "x" | "y" | "min-width" | "min-height" | "preferred-width"
        | "preferred-height" | "max-width" | "max-height" | "transform-origin-x"
        | "transform-origin-y" => Type::Length,
        "horizontal-stretch" | "vertical-stretch" | "z" | "opacity" | "transform-rotation"
        | "transform-scale" | "transform-scale-x" | "transform-scale-y" => Type::Float,
        "visible" => Type::Bool,
        "layout-order" | "row" | "col" | "rowspan" | "colspan" => Type::Int,
        "cross-axis-self-alignment" => Type::CrossAxisAlignment,
        _ => return None,
    })
}

fn linear_layout(kind: &str) -> bool {
    matches!(
        kind,
        "VerticalBox"
            | "HorizontalBox"
            | "VerticalLayout"
            | "HorizontalLayout"
            | "FlowLayout"
            | "FlexboxLayout"
            | "ToolBar"
            | "DialogButtonBox"
            | "PopupWindow"
            | "SwipeContent"
            | "SwipeLeft"
            | "SwipeRight"
            | "GroupBox"
            | "TimePicker"
            | "PageHeader"
            | "PageFooter"
    )
}

pub fn delegate_base(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "ItemDelegate" => "Button",
        "CheckDelegate" => "CheckBox",
        "RadioDelegate" => "RadioButton",
        "SwitchDelegate" => "Switch",
        _ => return None,
    })
}

pub fn is_builtin(kind: &str) -> bool {
    builtins::builtin_element(delegate_base(kind).unwrap_or(kind)).is_some()
}

pub fn media_read_type(name: &str) -> Option<Type> {
    Some(match name {
        "position" | "duration" | "buffer-progress" | "seek-start" | "seek-end" => Type::Float,
        "playing" | "ready" | "ended" | "seekable" | "has-video" | "has-audio" | "live"
        | "at-live-edge" => Type::Bool,
        "error" | "status" | "media-title" | "subtitle" | "audio-error" | "subtitle-error" => {
            Type::String
        }
        "audio-tracks" | "subtitle-tracks" | "bitrate" => Type::Int,
        _ => return None,
    })
}

fn text_input(kind: &str) -> bool {
    matches!(
        kind,
        "TextInput" | "LineEdit" | "SearchField" | "TextEdit" | "TextArea"
    )
}

fn focus_target_kind(kind: &str) -> bool {
    let kind = delegate_base(kind).unwrap_or(kind);
    text_input(kind)
        || toggle(kind)
        || matches!(
            kind,
            "FocusScope"
                | "Button"
                | "ToolButton"
                | "RoundButton"
                | "DelayButton"
                | "DialogButton"
                | "StandardButton"
                | "Slider"
                | "ScrollBar"
                | "Dial"
                | "SpinBox"
                | "DoubleSpinBox"
                | "ComboBox"
                | "Calendar"
                | "MonthGrid"
                | "RadioButton"
                | "Tab"
                | "TabButton"
                | "SwipeView"
                | "SwipeDelegate"
                | "StackView"
                | "ListView"
                | "GridView"
                | "StandardListView"
                | "TreeView"
                | "TableView"
                | "StandardTableView"
                | "GroupBox"
                | "PageIndicator"
                | "Tumbler"
        )
}

fn toggle(kind: &str) -> bool {
    matches!(kind, "CheckBox" | "Switch")
}

fn has_font(kind: &str) -> bool {
    text_input(kind)
        || toggle(kind)
        || matches!(
            kind,
            "Text"
                | "Button"
                | "ToolButton"
                | "RoundButton"
                | "DelayButton"
                | "DialogButton"
                | "GroupBox"
                | "Dialog"
                | "Drawer"
                | "TableCell"
                | "TreeCell"
                | "Tooltip"
                | "SpinBox"
                | "DoubleSpinBox"
                | "ComboBox"
                | "RadioButton"
                | "RadioGroup"
                | "Calendar"
                | "DatePickerPopup"
                | "Tumbler"
                | "Menu"
                | "MenuItem"
        )
}

fn picker_popup(kind: &str) -> bool {
    matches!(
        kind,
        "DatePickerPopup" | "TimePickerPopup" | "ColorPickerPopup" | "FontPickerPopup"
    )
}

fn time_picker(kind: &str) -> bool {
    matches!(kind, "TimePicker" | "TimePickerPopup")
}

fn table_view(kind: &str) -> bool {
    matches!(kind, "TableView" | "StandardTableView")
}

pub fn focus_property(kind: &str, property: &str) -> bool {
    property == "has-focus" && focus_target_kind(kind)
        || property == "focus-within" && kind == "FocusScope"
}

fn focus_handler(event: &str) -> bool {
    matches!(
        event,
        "focus-gained" | "focus-lost" | "focus-changed-event" | "focus-within-changed"
    )
}

fn key_handler(event: &str) -> bool {
    matches!(
        event,
        "capture-key-pressed" | "capture-key-released" | "key-pressed" | "key-released"
    )
}

fn control_event(kind: &str, event: &str) -> bool {
    let kind = delegate_base(kind).unwrap_or(kind);
    if matches!(
        kind,
        "SystemTrayIcon" | "Button" | "ToolButton" | "RoundButton"
    ) {
        return event == "clicked";
    }
    if kind == "SwipeGestureHandler" {
        return matches!(event, "moved" | "swiped" | "cancelled");
    }
    if kind == "HoverHandler" {
        return matches!(event, "entered" | "moved" | "exited");
    }
    if kind == "ScaleRotateGestureHandler" {
        return matches!(event, "started" | "updated" | "ended" | "cancelled");
    }
    if event == "scrolled" {
        return viewport_kind(kind) && kind != "Flickable";
    }
    if event == "flicked" {
        return kind == "Flickable";
    }
    if kind == "TreeView" {
        return matches!(
            event,
            "changed" | "expanded" | "collapsed" | "selection-changed"
        );
    }
    if kind == "SplitView" {
        return event == "resized";
    }
    matches!(kind, "TextInput" | "LineEdit") && matches!(event, "edited" | "accepted")
        || kind == "SearchField"
            && matches!(
                event,
                "edited" | "accepted" | "selected" | "search-triggered"
            )
        || matches!(kind, "TextEdit" | "TextArea") && event == "edited"
        || (toggle(kind) || kind == "GroupBox") && event == "toggled"
        || kind == "CheckBox" && event == "check-state-changed"
        || matches!(kind, "Slider" | "RangeSlider" | "Dial")
            && matches!(event, "changed" | "released")
        || kind == "ScrollBar" && event == "changed"
        || matches!(kind, "SpinBox" | "DoubleSpinBox") && event == "edited"
        || matches!(kind, "ComboBox" | "RadioGroup" | "ButtonGroup") && event == "selected"
        || kind == "ComboBox" && matches!(event, "edited" | "accepted" | "value-selected")
        || matches!(
            kind,
            "TabWidget"
                | "TabBar"
                | "SwipeView"
                | "PageIndicator"
                | "ListView"
                | "GridView"
                | "StandardListView"
                | "Tumbler"
        ) && event == "changed"
        || table_view(kind)
            && matches!(
                event,
                "current-row-changed"
                    | "current-cell-changed"
                    | "selection-changed"
                    | "sort-ascending"
                    | "sort-descending"
                    | "column-moved"
                    | "row-moved"
            )
        || kind == "Dialog"
            && matches!(
                event,
                "accepted" | "rejected" | "applied" | "reset" | "help-requested" | "discarded"
            )
        || matches!(kind, "Drawer" | "PopupWindow") && matches!(event, "opened" | "closed")
        || kind == "StackView" && event == "changed"
        || kind == "StackPage"
            && matches!(
                event,
                "leave-requested" | "activated" | "deactivated" | "removed"
            )
        || matches!(kind, "Calendar" | "MonthGrid") && matches!(event, "changed" | "month-changed")
        || kind == "MonthGrid" && event == "activated"
        || kind == "TimePicker" && event == "changed"
        || kind == "SwipeDelegate" && matches!(event, "clicked" | "changed")
        || kind == "StyledText" && event == "link-activated"
        || kind == "AnimatedImage" && event == "finished"
        || kind == "Timer" && event == "triggered"
        || kind == "FileDialog" && matches!(event, "accepted" | "canceled" | "failed")
        || kind == "MessageDialog"
            && matches!(event, "accepted" | "rejected" | "finished" | "failed")
        || is_window(kind) && event == "close-requested"
        || picker_popup(kind) && matches!(event, "accepted" | "canceled")
        || matches!(kind, "MenuItem" | "Action" | "ActionGroup" | "DelayButton")
            && event == "activated"
        || kind == "Shortcut" && matches!(event, "activated" | "activated-ambiguously")
        || kind == "FocusScope" && (key_handler(event) || focus_handler(event))
        || kind == "WheelHandler" && event == "wheel"
        || kind == "TouchArea"
            && matches!(
                event,
                "pointer-event" | "scroll-event" | "moved" | "double-clicked" | "press-and-hold"
            )
        || kind == "DragArea" && matches!(event, "drag-started" | "drag-finished")
        || kind == "DropArea"
            && matches!(
                event,
                "entered"
                    | "position-changed"
                    | "exited"
                    | "dropped"
                    | "files-entered"
                    | "files-dropped"
            )
}

/// Whether an element kind declares an event, independently of instance constraints.
pub fn builtin_event(kind: &str, event: &str) -> bool {
    let kind = delegate_base(kind).unwrap_or(kind);
    (matches!(kind, "StandardButton" | "TouchArea") && event == "clicked")
        || control_event(kind, event)
}

pub fn event_parameters<'a>(
    kind: &str,
    event: &str,
    combo_value: Option<&'a Type>,
    span: Span,
) -> Result<&'a [Type]> {
    let kind = delegate_base(kind).unwrap_or(kind);
    let scrolled = matches!(event, "scrolled" | "flicked");
    let key_handler = kind == "FocusScope" && key_handler(event);
    Ok(match kind {
        _ if scrolled => &[],
        _ if event == "current-cell-changed" => &[Type::Int, Type::Int],
        _ if event == "column-moved" => &[Type::Int, Type::Int],
        _ if event == "row-moved" => &[Type::Int, Type::Int, Type::Int],
        "FocusScope" if key_handler => &[Type::KeyEvent],
        "WheelHandler" if event == "wheel" => &[Type::WheelEvent],
        "TouchArea" if event == "scroll-event" => &[Type::WheelEvent],
        "TouchArea" if event == "pointer-event" => &[Type::PointerEvent],
        "FocusScope" if event == "focus-within-changed" => &[Type::Bool, Type::FocusReason],
        "FocusScope" if focus_handler(&event) => &[Type::FocusReason],
        "Slider" | "Dial" | "DoubleSpinBox" | "ScrollBar" => &[Type::Float],
        "RangeSlider" => &[Type::Float, Type::Float],
        "Calendar" | "MonthGrid" => &[Type::Date],
        "TimePicker" => &[Type::Time],
        "TimePickerPopup" if event == "accepted" => &[Type::Time],
        "DatePickerPopup" if event == "accepted" => &[Type::Date],
        "ColorPickerPopup" if event == "accepted" => &[Type::Color],
        "FontPickerPopup" if event == "accepted" => &[Type::Font],
        "FileDialog" if event == "accepted" => &[Type::StringList],
        "MessageDialog" if event == "finished" => &[Type::StandardButtonKind],
        kind if native_dialog(kind) && event == "failed" => &[Type::String],
        "GroupBox" => &[Type::Bool],
        "CheckBox" if event == "check-state-changed" => &[Type::CheckState],
        "StackView" => &[Type::String, Type::Int],
        "StackPage" if event == "leave-requested" => &[Type::String, Type::Bool],
        "SwipeDelegate" if event == "changed" => &[Type::SwipeSide],
        "StyledText" => &[Type::String],
        "DragArea" if event == "drag-finished" => &[Type::DragAction],
        "DropArea" if matches!(event, "entered" | "dropped") => &[Type::String, Type::DragAction],
        "DropArea" if event == "position-changed" => &[Type::Length, Type::Length],
        "DropArea" if matches!(event, "files-entered" | "files-dropped") => &[Type::StringList],
        "SpinBox" | "TabWidget" | "TabBar" | "SwipeView" | "PageIndicator" | "ListView"
        | "GridView" | "StandardListView" | "TableView" | "StandardTableView" | "TreeView"
        | "Tumbler" => &[Type::Int],
        "ComboBox" if event == "value-selected" => {
            let Some(ty) = combo_value else {
                return fail(span, "value-selected requires a record ComboBox model");
            };
            std::slice::from_ref(ty)
        }
        "ComboBox" | "RadioGroup" | "ButtonGroup" | "ActionGroup" | "SearchField" => {
            &[Type::String]
        }
        "TextEdit" | "TextArea" => &[Type::String],
        _ => &[],
    })
}

pub fn event_return_type(kind: &str, event: &str) -> Option<Type> {
    if matches!(
        (kind, event),
        ("Window" | "ApplicationWindow", "close-requested") | ("StackPage", "leave-requested")
    ) {
        Some(Type::Bool)
    } else if kind == "FocusScope" && key_handler(event)
        || matches!(
            (kind, event),
            ("WheelHandler", "wheel") | ("TouchArea", "scroll-event")
        )
    {
        Some(Type::EventResult)
    } else {
        None
    }
}

/// Validated members of one source component, in lowering order.
pub enum LoweredMember<'a> {
    Property(ir::Property),
    Callback(ir::Callback),
    InstanceHandler(ir::Callback),
    ClipboardPaste(ir::ClipboardPaste),
    BeginElement {
        kind: &'a str,
        delegate: Option<&'a str>,
        id: Option<&'a str>,
        key: String,
        instance: bool,
        span: Span,
    },
    Binding(ir::Binding),
    ActionRef {
        owner: String,
        span: Span,
    },
    ActionGroupRef {
        membership: ir::ActionGroupRef,
        span: Span,
    },
    ScrollTarget {
        target: String,
        span: Span,
    },
    SplitFill {
        target: String,
        span: Span,
    },
    RadioGroupRef {
        group: String,
        span: Span,
    },
    CheckGroupRef {
        group: String,
        property: &'static str,
        span: Span,
    },
    InstanceLayout(ir::Binding),
    Condition(ir::Expr),
    ControlLink(ir::Binding),
    InstanceLink(ir::Binding),
    RowLink(ir::Binding),
    Animation(ir::Animation),
    StateSelection(ir::StateSelection),
    StateBinding(ir::StateBinding),
    StateTiming(ir::StateTiming),
    InitialFocus {
        target: String,
        span: Span,
    },
    Repeat {
        model: ir::Expr,
        item: String,
        index: Option<String>,
    },
    Handler(ir::Callback),
    EndElement,
}

/// A source reference resolved by the same rules that lower the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolReference {
    pub usage: Span,
    pub spelling: String,
    /// Index of the referenced identifier in a dotted name.
    pub segment: usize,
    pub declaration: Span,
    pub name: String,
}

type SymbolReferences = std::rc::Rc<std::cell::RefCell<Vec<SymbolReference>>>;

pub fn resolve_symbols(
    doc: &ast::Document,
) -> std::result::Result<Vec<SymbolReference>, Vec<Diagnostic>> {
    let references = SymbolReferences::default();
    analyze_components(doc, Some(references.clone()), |_, _| {})?;
    Ok(std::rc::Rc::try_unwrap(references)
        .expect("analysis scopes released")
        .into_inner())
}

/// Hand off each component only after its names, types and bindings are valid.
pub fn for_each_component(
    doc: &ast::Document,
    emit: impl FnMut(&ast::Component, Vec<LoweredMember<'_>>),
) -> std::result::Result<(), Vec<Diagnostic>> {
    analyze_components(doc, None, emit)
}

fn analyze_components(
    doc: &ast::Document,
    references: Option<SymbolReferences>,
    mut emit: impl FnMut(&ast::Component, Vec<LoweredMember<'_>>),
) -> std::result::Result<(), Vec<Diagnostic>> {
    if !doc.imports.is_empty() {
        return Err(vec![Diagnostic {
            span: doc.imports[0].span,
            message: "resolve imports before semantic analysis".into(),
        }]);
    }
    let records = validate_records(doc).map_err(|error| vec![error])?;
    let definitions: BTreeMap<_, _> = doc
        .components
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();
    if let Some(references) = &references {
        for record in &doc.records {
            for field in &record.fields {
                if let Type::Record(name) = &field.ty
                    && let Some(target) = records.get(name.as_ref())
                {
                    references.borrow_mut().push(SymbolReference {
                        usage: field.type_span,
                        spelling: String::new(),
                        segment: 0,
                        declaration: target.span,
                        name: target.name.clone(),
                    });
                }
            }
        }
        for component in &doc.components {
            if let Some(base) = definitions.get(component.base.as_str()) {
                references.borrow_mut().push(SymbolReference {
                    usage: component.base_span,
                    spelling: String::new(),
                    segment: 0,
                    declaration: base.span,
                    name: base.name.clone(),
                });
            }
        }
    }
    let mut inherited = BTreeMap::new();
    let mut active = BTreeSet::new();
    let components = doc
        .components
        .iter()
        .map(|c| {
            inherit(
                c,
                &definitions,
                &mut inherited,
                &mut active,
                references.as_ref(),
            )
        })
        .collect::<Result<Vec<_>>>()
        .map_err(|error| vec![error])?;
    let mut names = BTreeSet::new();
    let mut definitions = BTreeMap::new();
    let mut errors = Vec::new();
    for c in &components {
        if !names.insert(rust_name(&c.name)) {
            errors.push(Diagnostic {
                span: c.span,
                message: format!("duplicate component `{}`", c.name),
            });
        } else {
            definitions.insert(c.name.as_str(), c);
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let placed = crate::slots::place(components).map_err(|error| vec![error])?;
    let definitions: BTreeMap<_, _> = placed
        .iter()
        .map(|placed| (placed.component.name.as_str(), &placed.component))
        .collect();
    let mut scopes = BTreeMap::new();
    for placed in &placed {
        let c = &placed.component;
        let mut scope = Scope {
            records: records.clone(),
            component_name: &c.name,
            global: c.global,
            placement_names: &placed.scopes,
            references: references.clone(),
            ..Scope::default()
        };
        for (&name, definition) in &definitions {
            if !definition.global || name == c.name {
                continue;
            }
            let global = format!("__global_{name}");
            if let Err(error) = scope.register(
                &definitions,
                &[],
                name,
                &global,
                Some("root"),
                definition.span,
                None,
            ) {
                errors.push(error);
            }
        }
        match scope.register(
            &definitions,
            &c.members,
            &c.base,
            "root",
            None,
            c.span,
            None,
        ) {
            Ok(()) => {
                scopes.insert(c.name.as_str(), scope);
            }
            Err(error) => errors.push(error),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let callbacks = std::rc::Rc::new(resolve_callbacks(&scopes).map_err(|error| vec![error])?);
    let mut validated = Vec::new();
    for placed in &placed {
        let c = &placed.component;
        let mut scope = scopes
            .remove(c.name.as_str())
            .expect("registered component scope");
        scope.callback_catalog = Some(callbacks.clone());
        match ActionGroups::resolve(&scope.owners) {
            Ok(groups) => scope.action_groups = std::rc::Rc::new(groups),
            Err(error) => {
                errors.push(error);
                continue;
            }
        }
        match component(c, scope) {
            Ok(members) => validated.push((c, members)),
            Err(e) => errors.push(e),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    check_composition(&validated).map_err(|error| vec![error])?;
    for (c, members) in validated {
        emit(c, members);
    }
    Ok(())
}

fn validate_records(doc: &ast::Document) -> Result<BTreeMap<&str, &ast::RecordDef>> {
    let mut names: BTreeSet<_> = doc.components.iter().map(|c| rust_name(&c.name)).collect();
    let mut records = BTreeMap::new();
    for record in &doc.records {
        check_name(&record.name, record.span)?;
        if Type::parse(&record.name).is_some()
            || is_builtin(&record.name)
            || !names.insert(rust_name(&record.name))
        {
            return fail(
                record.span,
                format!("duplicate or reserved record `{}`", record.name),
            );
        }
        let mut fields = BTreeSet::new();
        for field in &record.fields {
            check_name(&field.name, field.span)?;
            if !fields.insert(rust_name(&field.name)) {
                return fail(
                    field.span,
                    format!("duplicate record field `{}`", field.name),
                );
            }
            if matches!(
                field.ty,
                Type::RecordList(_)
                    | Type::StringList
                    | Type::TouchPoints
                    | Type::GradientStop
                    | Type::Percent
            ) {
                return fail(
                    field.span,
                    "record fields require scalar, enum or named record types",
                );
            }
        }
        if fields.is_empty() {
            return fail(record.span, "record must declare at least one field");
        }
        records.insert(record.name.as_str(), record);
    }
    let mut layouts = Dependencies::new();
    for record in &doc.records {
        let mut fields = Vec::new();
        for field in &record.fields {
            if let Type::Record(name) = &field.ty {
                if !records.contains_key(name.as_ref()) {
                    return fail(field.span, format!("unknown record type `{name}`"));
                }
                fields.push(name.to_string());
            }
        }
        layouts.insert(record.name.clone(), (record.span, fields));
    }
    check_cycles(&layouts, "record layout")?;
    Ok(records)
}

fn record_field_type(
    records: &BTreeMap<&str, &ast::RecordDef>,
    record: &str,
    path: &str,
) -> Option<Type> {
    let mut fields = path.split('.').peekable();
    let mut schema = *records.get(record)?;
    while let Some(name) = fields.next() {
        let field = schema.fields.iter().find(|field| field.name == name)?;
        if fields.peek().is_none() {
            return Some(field.ty.clone());
        }
        let Type::Record(next) = &field.ty else {
            return None;
        };
        schema = *records.get(next.as_ref())?;
    }
    None
}

fn inherit<'a>(
    component: &'a ast::Component,
    definitions: &BTreeMap<&str, &'a ast::Component>,
    expanded: &mut BTreeMap<&'a str, ast::Component>,
    active: &mut BTreeSet<&'a str>,
    references: Option<&SymbolReferences>,
) -> Result<ast::Component> {
    if is_builtin(&component.base) {
        return Ok(component.clone());
    }
    if let Some(previous) = expanded.get(component.name.as_str()) {
        return Ok(previous.clone());
    }
    if !active.insert(&component.name) {
        return fail(
            component.span,
            format!(
                "cyclic component inheritance involving `{}`",
                component.name
            ),
        );
    }
    let base = definitions
        .get(component.base.as_str())
        .ok_or_else(|| Diagnostic {
            span: component.base_span,
            message: format!("unknown base element or component `{}`", component.base),
        })?;
    if base.global {
        return fail(component.base_span, "a component cannot inherit a global");
    }
    let base = inherit(base, definitions, expanded, active, references)?;
    let mut result = component.clone();
    result.base = base.base;
    result.members = base.members;
    let mut bindings = BTreeSet::new();
    let mut handlers = BTreeSet::new();
    for member in &component.members {
        match member {
            Member::Binding(binding) => {
                if !bindings.insert(&binding.name) {
                    return fail(
                        binding.span,
                        format!("duplicate binding `{}`", binding.name),
                    );
                }
                if let Some(property) = result.members.iter_mut().find_map(|member| match member {
                    Member::Property(property) if property.name == binding.name => Some(property),
                    _ => None,
                }) {
                    if binding.two_way
                        || property.two_way
                        || !matches!(property.visibility.as_str(), "in" | "in-out")
                    {
                        return fail(
                            binding.span,
                            format!("cannot override inherited property `{}`", binding.name),
                        );
                    }
                    if let Some(references) = references {
                        references.borrow_mut().push(SymbolReference {
                            usage: binding.span,
                            spelling: binding.name.clone(),
                            segment: 0,
                            declaration: property.span,
                            name: property.name.clone(),
                        });
                    }
                    property.value = Some(binding.value.clone());
                    continue;
                }
                result.members.retain(|member| !matches!(member, Member::Binding(existing) if existing.name == binding.name));
            }
            Member::Handler(handler) if handlers.insert(&handler.name) => {
                result.members.retain(|member| !matches!(member, Member::Handler(existing) if existing.name == handler.name));
            }
            Member::Animation(animation) => {
                result.members.retain(|member| !matches!(member, Member::Animation(existing) if existing.property == animation.property));
            }
            _ => {}
        }
        result.members.push(member.clone());
    }
    active.remove(component.name.as_str());
    expanded.insert(&component.name, result.clone());
    Ok(result)
}

#[derive(Clone)]
struct Owner<'a> {
    properties: BTreeMap<&'a str, &'a ast::Property>,
    callbacks: BTreeMap<&'a str, &'a ast::Callback>,
    parent: Option<String>,
    instance: bool,
    placement: Option<&'a ast::PlacementScope>,
    kind: String,
    members: &'a [Member],
    repeated: bool,
    span: Span,
}

#[derive(Clone, Default)]
struct StateTarget {
    bindings: Vec<ir::StateBinding>,
    timings: Vec<ir::StateTiming>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowTemplate {
    Table,
    Tree,
    List,
}

#[derive(Clone, Default)]
struct Scope<'a> {
    owners: BTreeMap<String, Owner<'a>>,
    parameters: BTreeMap<String, Type>,
    parameter_sites: BTreeMap<String, Span>,
    effects_allowed: bool,
    writable_row: Option<String>,
    row_template: Option<RowTemplate>,
    repeat_source: Option<RepeatSource>,
    records: BTreeMap<&'a str, &'a ast::RecordDef>,
    state_targets: BTreeMap<String, StateTarget>,
    component_name: &'a str,
    global: bool,
    placement_names: &'a [crate::slots::Names],
    caller_scope: Option<(&'a ast::PlacementScope, String)>,
    callback_catalog: Option<std::rc::Rc<CallbackCatalog>>,
    action_groups: std::rc::Rc<ActionGroups>,
    references: Option<SymbolReferences>,
}

#[derive(Default)]
struct ActionGroups {
    members: BTreeMap<String, ir::ActionGroupRef>,
    initial: BTreeMap<String, String>,
}

impl ActionGroups {
    fn resolve(owners: &BTreeMap<String, Owner<'_>>) -> Result<Self> {
        let mut groups = Self::default();
        let mut values = BTreeMap::<&str, BTreeSet<String>>::new();
        for (id, action) in owners.iter().filter(|(_, action)| action.kind == "Action") {
            let attachment = named_binding(action.members, "group");
            let inline = action
                .parent
                .as_deref()
                .filter(|parent| owners[*parent].kind == "ActionGroup");
            let group = if let Some(binding) = attachment {
                if inline.is_some() {
                    return fail(
                        binding.span,
                        "Action already belongs to its enclosing ActionGroup",
                    );
                }
                let ExprKind::Reference(group) = &binding.value.kind else {
                    return fail(binding.span, "group requires the id of an ActionGroup");
                };
                if binding.two_way
                    || !owners
                        .get(group)
                        .is_some_and(|owner| owner.kind == "ActionGroup")
                {
                    return fail(binding.span, "group requires the id of an ActionGroup");
                }
                group.as_str()
            } else if let Some(group) = inline {
                group
            } else {
                continue;
            };
            let value_binding = named_binding(action.members, "value");
            let value = if let Some(binding) = value_binding {
                let ExprKind::Literal {
                    value,
                    ty: Type::String,
                } = &binding.value.kind
                else {
                    return fail(
                        binding.span,
                        "Action value requires a nonempty string literal",
                    );
                };
                let value: String = serde_json::from_str(value).map_err(|error| Diagnostic {
                    span: binding.span,
                    message: error.to_string(),
                })?;
                if binding.two_way || value.is_empty() {
                    return fail(
                        binding.span,
                        "Action value requires a nonempty string literal",
                    );
                }
                value
            } else {
                id.clone()
            };
            if !values.entry(group).or_default().insert(value.clone()) {
                return fail(
                    value_binding.map_or(action.span, |binding| binding.span),
                    format!("duplicate ActionGroup value `{value}`"),
                );
            }
            let group_members = owners[group].members;
            if !non_exclusive_group(group_members)
                && let Some(binding) = named_binding(action.members, "checked")
            {
                let ExprKind::Literal {
                    value: checked,
                    ty: Type::Bool,
                } = &binding.value.kind
                else {
                    return fail(
                        binding.span,
                        "exclusive Action checked requires a literal; bind ActionGroup current-value for shared state",
                    );
                };
                if binding.two_way {
                    return fail(
                        binding.span,
                        "bind ActionGroup current-value for shared state",
                    );
                }
                if checked == "true"
                    && (named_binding(group_members, "current-value").is_some()
                        || groups.initial.insert(group.into(), value.clone()).is_some())
                {
                    return fail(
                        binding.span,
                        "exclusive ActionGroup requires a single initial selection",
                    );
                }
            }
            groups.members.insert(
                id.clone(),
                ir::ActionGroupRef {
                    group: group.into(),
                    value,
                },
            );
        }
        Ok(groups)
    }
}

#[derive(Clone)]
struct RepeatSource {
    item: String,
    owner: String,
    property: Option<(String, Span)>,
}

type CallbackKey = (String, String, String);
type CallbackCatalog = BTreeMap<CallbackKey, ResolvedCallback>;

fn callback_key(component: &str, owner: &str, name: &str) -> CallbackKey {
    (component.into(), owner.into(), name.into())
}

#[derive(Clone)]
struct ResolvedCallback {
    signature: ast::Callback,
    target: Option<ir::CallbackRef>,
    builtin: bool,
}

fn resolve_callbacks(scopes: &BTreeMap<&str, Scope<'_>>) -> Result<CallbackCatalog> {
    let mut resolver = CallbackResolver {
        scopes,
        resolved: BTreeMap::new(),
        active: BTreeSet::new(),
    };
    for (&component, scope) in scopes {
        for (owner, element) in &scope.owners {
            if element.instance {
                continue;
            }
            for (&name, declaration) in &element.callbacks {
                resolver.resolve(component, owner, name, declaration.span)?;
            }
        }
    }
    Ok(resolver.resolved)
}

struct CallbackResolver<'a, 'b> {
    scopes: &'a BTreeMap<&'b str, Scope<'b>>,
    resolved: CallbackCatalog,
    active: BTreeSet<CallbackKey>,
}

impl CallbackResolver<'_, '_> {
    fn resolve(
        &mut self,
        component: &str,
        owner: &str,
        name: &str,
        span: Span,
    ) -> Result<ResolvedCallback> {
        let key = callback_key(component, owner, name);
        if let Some(resolved) = self.resolved.get(&key) {
            return Ok(resolved.clone());
        }
        if !self.active.insert(key.clone()) {
            return fail(
                span,
                format!("cyclic callback alias involving `{owner}.{name}`"),
            );
        }
        let scope = &self.scopes[component];
        let element = &scope.owners[owner];
        let callback = if let Some(declaration) = element.callbacks.get(name) {
            let mut signature = (*declaration).clone();
            let target = if let Some(reference) = &declaration.alias {
                let (target, target_name) = scope.member_owner(
                    owner,
                    reference,
                    "callback",
                    declaration.span,
                    |scope, name| scope.callbacks.contains_key(name),
                )?;
                if target.starts_with("__global_") {
                    return fail(
                        declaration.span,
                        "global callbacks can be called but cannot be aliased",
                    );
                }
                for start in [owner, target] {
                    let mut current = Some(start);
                    while let Some(key) = current {
                        let element = &scope.owners[key];
                        if element.repeated {
                            return fail(
                                declaration.span,
                                "callback aliases require statically addressed endpoints",
                            );
                        }
                        current = element.parent.as_deref();
                    }
                }
                let receiver = &scope.owners[target];
                if receiver.instance && !receiver.callbacks.contains_key(target_name) {
                    return fail(declaration.span, format!("unknown callback `{reference}`"));
                }
                if let Some(endpoint) = receiver.callbacks.get(target_name)
                    && let Some(span) = declaration.alias_span
                {
                    scope.member_reference(span, reference, target, endpoint.span, target_name);
                }
                let (target_component, target_owner) = if receiver.instance {
                    (receiver.kind.as_str(), "root")
                } else {
                    (component, target)
                };
                let resolved = self.resolve(
                    target_component,
                    target_owner,
                    target_name,
                    declaration.span,
                )?;
                if declaration.pure && !resolved.signature.pure {
                    return fail(
                        declaration.span,
                        "a pure alias requires a pure callback target",
                    );
                }
                signature.parameters = resolved.signature.parameters;
                signature.return_type = resolved.signature.return_type;
                signature.pure = resolved.signature.pure;
                Some(ir::CallbackRef {
                    owner: target.into(),
                    name: target_name.into(),
                })
            } else {
                None
            };
            ResolvedCallback {
                signature,
                target,
                builtin: false,
            }
        } else if builtin_event(&element.kind, name) {
            if matches!(
                element.kind.as_str(),
                "Button" | "ToolButton" | "RoundButton" | "MenuItem" | "Shortcut"
            ) && element.members.iter().any(
                |member| matches!(member, Member::Binding(binding) if binding.name == "action"),
            ) {
                return fail(span, "alias the linked Action callback");
            }
            let combo = if element.kind == "ComboBox" && name == "value-selected" {
                record_combo(element.members, scope, owner)?
            } else {
                None
            };
            let types = event_parameters(
                &element.kind,
                name,
                combo.as_ref().map(|combo| &combo.value_type),
                span,
            )?;
            ResolvedCallback {
                signature: ast::Callback {
                    name: name.into(),
                    parameters: types
                        .iter()
                        .cloned()
                        .map(|ty| ast::Parameter {
                            name: String::new(),
                            ty,
                        })
                        .collect(),
                    span,
                    return_type: event_return_type(&element.kind, name),
                    pure: false,
                    alias: None,
                    alias_span: None,
                    parameter_type_spans: vec![],
                    result_type_span: None,
                },
                target: None,
                builtin: true,
            }
        } else {
            return fail(span, format!("unknown callback `{owner}.{name}`"));
        };
        self.active.remove(&key);
        self.resolved.insert(key, callback.clone());
        Ok(callback)
    }
}

fn key(e: &ast::Element, parent: &str, index: usize) -> String {
    if let Some(placement) = &e.placement_scope {
        return placement.key.clone();
    }
    match &e.id {
        Some(id) => id.clone(),
        None => format!("__{parent}_{index}"),
    }
}

fn check_element_type(
    definitions: &BTreeMap<&str, &ast::Component>,
    element: &ast::Element,
) -> Result<()> {
    if !is_builtin(&element.ty) && !definitions.contains_key(element.ty.as_str()) {
        return fail(
            element.type_span,
            format!("unknown element or component `{}`", element.ty),
        );
    }
    Ok(())
}

fn contains_element<'a>(
    kind: &'a str,
    members: &'a [Member],
    definitions: &BTreeMap<&str, &'a ast::Component>,
    visited: &mut BTreeSet<&'a str>,
    matches_kind: impl Fn(&str) -> bool + Copy,
) -> bool {
    if matches_kind(kind) {
        return true;
    }
    if let Some(component) = definitions.get(kind)
        && !is_builtin(kind)
        && visited.insert(kind)
        && contains_element(
            &component.base,
            &component.members,
            definitions,
            visited,
            matches_kind,
        )
    {
        return true;
    }
    members.iter().any(|member| {
        matches!(member, Member::Element(child)
        if contains_element(&child.ty, &child.members, definitions, visited, matches_kind))
    })
}

fn validate_table(kind: &str, members: &[Member], span: Span) -> Result<()> {
    let standard = kind == "StandardTableView";
    let mut columns = 0;
    let mut row = None;
    for element in members.iter().filter_map(|member| match member {
        Member::Element(element) if !nonvisual_element(&element.ty) => Some(element),
        _ => None,
    }) {
        match element.ty.as_str() {
            "TableColumn" if element.repeat.is_none() && element.condition.is_none() => {
                if element
                    .members
                    .iter()
                    .any(|member| !matches!(member, Member::Binding(_)))
                {
                    return fail(element.span, "TableColumn only supports property bindings");
                }
                if !standard && element.members.iter().any(|member| {
                    matches!(member, Member::Binding(binding) if binding.name == "field")
                }) {
                    return fail(element.span, "TableColumn.field requires StandardTableView");
                }
                if !standard && element.members.iter().any(|member| {
                    matches!(member, Member::Binding(binding) if binding.name == "editable")
                }) {
                    return fail(element.span, "TableColumn.editable requires StandardTableView");
                }
                columns += 1;
            }
            "TableRow"
                if !standard
                    && row.is_none()
                    && element.repeat.is_some()
                    && element.condition.is_none() =>
            {
                row = Some(element);
            }
            _ => {
                if standard {
                    return fail(
                        element.span,
                        "StandardTableView accepts static TableColumn children",
                    );
                }
                return fail(
                    element.span,
                    "TableView requires static TableColumn children and one repeated TableRow",
                );
            }
        }
    }
    if standard {
        if columns == 0
            || !members
                .iter()
                .any(|member| matches!(member, Member::Binding(binding) if binding.name == "model"))
        {
            return fail(
                span,
                "StandardTableView requires a record model and at least one TableColumn",
            );
        }
        return Ok(());
    }
    let Some(row) = row.filter(|_| columns > 0) else {
        return fail(
            span,
            "TableView requires at least one TableColumn and one repeated TableRow",
        );
    };
    let cells = row.members.iter().filter_map(|member| match member {
        Member::Element(cell) => Some(cell),
        _ => None,
    });
    if cells.clone().count() != columns || cells.into_iter().any(|cell| cell.repeat.is_some()) {
        return fail(row.span, "TableRow requires one cell element per column");
    }
    fn parsers(members: &[Member]) -> usize {
        members
            .iter()
            .map(|member| match member {
                Member::Handler(handler) if handler.name == "clipboard-paste" => 1,
                Member::Element(element) => parsers(&element.members),
                _ => 0,
            })
            .sum()
    }
    for member in &row.members {
        if let Member::Element(cell) = member
            && parsers(&cell.members) > 1
        {
            return fail(
                cell.span,
                "each table column accepts only one clipboard-paste handler",
            );
        }
    }
    Ok(())
}

fn swipe_slot(kind: &str) -> bool {
    matches!(kind, "SwipeContent" | "SwipeLeft" | "SwipeRight")
}

fn nonvisual_element(kind: &str) -> bool {
    matches!(
        kind,
        "Action" | "ActionGroup" | "ButtonGroup" | "Timer" | "SecondaryWindow"
    ) || native_dialog(kind)
}

fn exclusive_binding(members: &[Member]) -> Option<&ast::Binding> {
    named_binding(members, "exclusive")
}

fn named_binding<'a>(members: &'a [Member], name: &str) -> Option<&'a ast::Binding> {
    members.iter().find_map(|member| match member {
        Member::Binding(binding) if binding.name == name => Some(binding),
        _ => None,
    })
}

fn non_exclusive_group(members: &[Member]) -> bool {
    exclusive_binding(members).is_some_and(|binding| {
        matches!(&binding.value.kind,
        ExprKind::Literal { value, ty: Type::Bool } if value == "false")
    })
}

fn native_dialog(kind: &str) -> bool {
    matches!(kind, "FileDialog" | "MessageDialog")
}

pub fn native_dialog_property(kind: &str, name: &str) -> Option<Type> {
    match (kind, name) {
        ("FileDialog" | "MessageDialog", "pending") => Some(Type::Bool),
        ("FileDialog", "selected-file") => Some(Type::String),
        ("FileDialog", "selected-files") => Some(Type::StringList),
        _ => None,
    }
}

fn validate_swipe_delegate(members: &[Member], span: Span) -> Result<()> {
    let mut slots = BTreeSet::new();
    for member in members {
        let Member::Element(element) = member else {
            continue;
        };
        if nonvisual_element(&element.ty) {
            continue;
        }
        if !swipe_slot(&element.ty) || element.condition.is_some() || element.repeat.is_some() {
            return fail(
                element.span,
                "SwipeDelegate requires static SwipeContent, SwipeLeft or SwipeRight slots",
            );
        }
        if !slots.insert(element.ty.as_str()) {
            return fail(element.span, format!("duplicate {} slot", element.ty));
        }
    }
    if slots.len() < 2 || !slots.contains("SwipeContent") {
        return fail(
            span,
            "SwipeDelegate requires SwipeContent and at least one action slot",
        );
    }
    Ok(())
}

impl<'a> Scope<'a> {
    fn reference(
        &self,
        usage: Span,
        spelling: &str,
        segment: usize,
        declaration: Span,
        name: &str,
    ) {
        if let Some(references) = &self.references {
            references.borrow_mut().push(SymbolReference {
                usage,
                spelling: spelling.into(),
                segment,
                declaration,
                name: name.into(),
            });
        }
    }

    fn member_reference(
        &self,
        usage: Span,
        spelling: &str,
        owner: &str,
        declaration: Span,
        name: &str,
    ) {
        if let Some((receiver, _)) = spelling.split_once('.') {
            if !matches!(receiver, "root" | "self") {
                let source = receiver
                    .split_once('@')
                    .map_or(receiver, |(source, _)| source);
                let spelling = format!("{source}.{}", spelling.split_once('.').unwrap().1);
                self.reference(usage, &spelling, 0, self.owners[owner].span, source);
                self.reference(usage, &spelling, 1, declaration, name);
                return;
            }
            self.reference(usage, spelling, 1, declaration, name);
        } else {
            self.reference(usage, spelling, 0, declaration, name);
        }
    }

    fn record_handler_parameters(&mut self, handler: &ast::Handler) {
        if self.references.is_some() {
            self.parameter_sites.extend(
                handler
                    .parameters
                    .iter()
                    .cloned()
                    .zip(handler.parameter_spans.iter().copied()),
            );
        }
    }

    fn callback_info(&self, owner: &str, name: &str) -> Option<&ResolvedCallback> {
        let element = self.owners.get(owner)?;
        let (component, owner) = if element.instance {
            if !element.callbacks.contains_key(name) {
                return None;
            }
            (element.kind.as_str(), "root")
        } else {
            (self.component_name, owner)
        };
        self.callback_catalog
            .as_ref()?
            .get(&callback_key(component, owner, name))
    }

    fn local_callbacks<'s>(&'s self, owner: &'s str) -> impl Iterator<Item = &'s ResolvedCallback> {
        self.callback_catalog
            .iter()
            .flat_map(|catalog| catalog.iter())
            .filter_map(move |((component, key, _), resolved)| {
                (component == self.component_name && key == owner).then_some(resolved)
            })
    }

    fn record_type_reference(&self, ty: &Type, usage: Span) {
        if let Type::Record(name) | Type::RecordList(name) = ty
            && let Some(record) = self.records.get(name.as_ref())
        {
            self.reference(usage, "", 0, record.span, &record.name);
        }
    }

    fn checked_type(&self, ty: &Type, span: Span) -> Result<()> {
        if matches!(ty, Type::Percent | Type::GradientStop) {
            return fail(span, format!("{} is an internal value type", ty.name()));
        }
        if let Type::Record(name) | Type::RecordList(name) = ty
            && !self.records.contains_key(name.as_ref())
        {
            return fail(span, format!("unknown record type `{name}`"));
        }
        Ok(())
    }
    // Register declarations first so bindings may reference later siblings.
    fn register(
        &mut self,
        definitions: &BTreeMap<&str, &'a ast::Component>,
        members: &'a [Member],
        kind: &str,
        owner: &str,
        parent: Option<&str>,
        span: Span,
        placement: Option<&'a ast::PlacementScope>,
    ) -> Result<()> {
        if matches!(kind, "ActionGroup" | "ButtonGroup")
            && let Some(binding) = exclusive_binding(members)
            && (binding.two_way
                || !matches!(binding.value.kind, ExprKind::Literal { ty: Type::Bool, .. }))
        {
            return fail(
                binding.span,
                format!("{kind} exclusive requires a literal bool"),
            );
        }
        let instance = !is_builtin(kind);
        if parent.is_some()
            && (kind == "SystemTrayIcon"
                || definitions
                    .get(kind)
                    .is_some_and(|definition| definition.base == "SystemTrayIcon"))
        {
            return fail(span, "SystemTrayIcon requires a top-level component");
        }
        let property_members = if instance {
            &definitions
                .get(kind)
                .ok_or_else(|| Diagnostic {
                    span,
                    message: format!("unknown element or component `{kind}`"),
                })?
                .members
        } else {
            members
        };
        let mut properties = BTreeMap::new();
        let mut callbacks = BTreeMap::new();
        let mut names = BTreeSet::new();
        for p in property_members.iter().filter_map(as_property) {
            check_name(&p.name, p.span)?;
            self.checked_type(&p.ty, p.span)?;
            self.record_type_reference(&p.ty, p.type_span);
            if !names.insert(rust_name(&p.name)) || builtin_property(kind, &p.name).is_some() {
                return fail(
                    p.span,
                    format!("duplicate or conflicting member `{}`", p.name),
                );
            }
            properties.insert(p.name.as_str(), p);
        }
        for callback in property_members.iter().filter_map(|member| {
            if let Member::Callback(callback) = member {
                Some(callback)
            } else {
                None
            }
        }) {
            check_name(&callback.name, callback.span)?;
            let mut parameters = BTreeSet::new();
            for (index, parameter) in callback.parameters.iter().enumerate() {
                if let Some(span) = callback.parameter_type_spans.get(index) {
                    self.record_type_reference(&parameter.ty, *span);
                }
                self.checked_type(&parameter.ty, callback.span)?;
                if !parameter.name.is_empty() {
                    check_name(&parameter.name, callback.span)?;
                    if !parameters.insert(rust_name(&parameter.name)) {
                        return fail(
                            callback.span,
                            format!("duplicate parameter `{}`", parameter.name),
                        );
                    }
                }
            }
            if let Some(ty) = &callback.return_type {
                self.checked_type(ty, callback.span)?;
                if let Some(span) = callback.result_type_span {
                    self.record_type_reference(ty, span);
                }
            }
            if builtin_property(kind, &callback.name).is_some()
                || builtin_event(kind, &callback.name)
                || !names.insert(rust_name(&callback.name))
            {
                return fail(
                    callback.span,
                    format!("duplicate or conflicting member `{}`", callback.name),
                );
            }
            callbacks.insert(callback.name.as_str(), callback);
        }
        if self
            .owners
            .insert(
                owner.into(),
                Owner {
                    properties,
                    callbacks,
                    parent: parent.map(str::to_owned),
                    instance,
                    placement,
                    kind: kind.into(),
                    members,
                    span,
                    repeated: false,
                },
            )
            .is_some()
        {
            return fail(span, format!("duplicate element id `{owner}`"));
        }
        if !instance {
            if kind == "ApplicationWindow" && parent.is_some() {
                return fail(span, "ApplicationWindow requires a component root");
            }
            if kind == "ToolSeparator"
                && members
                    .iter()
                    .any(|member| matches!(member, Member::Element(_)))
            {
                return fail(span, "ToolSeparator cannot contain child elements");
            }
            if kind == "SystemTrayIcon" {
                let mut menus = 0;
                for element in members.iter().filter_map(|member| match member {
                    Member::Element(element) => Some(element),
                    _ => None,
                }) {
                    if element.ty == "Menu"
                        && element.repeat.is_none()
                        && element.condition.is_none()
                    {
                        menus += 1;
                    } else if element.ty != "Action" {
                        return fail(
                            element.span,
                            "SystemTrayIcon accepts one optional static Menu and named Actions",
                        );
                    }
                }
                if menus > 1 {
                    return fail(span, "SystemTrayIcon accepts at most one Menu");
                }
            }
            if kind == "SecondaryWindow" {
                let mut children = members.iter().filter_map(|member| match member {
                    Member::Element(child) => Some(child),
                    _ => None,
                });
                if !children.next().is_some_and(|child| {
                    child.repeat.is_none()
                        && child.condition.is_none()
                        && definitions
                            .get(child.ty.as_str())
                            .is_some_and(|definition| is_window(&definition.base))
                }) || children.next().is_some()
                {
                    return fail(
                        span,
                        "SecondaryWindow requires one static instance of a Window component",
                    );
                }
            }
            let tooltips = members
                .iter()
                .filter(
                    |member| matches!(member, Member::Element(element) if element.ty == "Tooltip"),
                )
                .count();
            if tooltips > 1 {
                return fail(span, "an element can contain at most one Tooltip");
            }
            if kind == "Tooltip" {
                let mut children = members.iter().filter_map(|member| match member {
                    Member::Element(element) if !nonvisual_element(&element.ty) => Some(element),
                    _ => None,
                });
                if let Some(content) = children.next() {
                    if children.next().is_some() || members.iter().any(|member| matches!(member, Member::Binding(binding) if binding.name == "text")) {
                        return fail(span, "Tooltip requires text or a single content child");
                    }
                    if contains_element(
                        &content.ty,
                        &content.members,
                        definitions,
                        &mut BTreeSet::new(),
                        |kind| {
                            matches!(
                                kind,
                                "Tooltip"
                                    | "Dialog"
                                    | "Drawer"
                                    | "PopupWindow"
                                    | "TimePickerPopup"
                                    | "DatePickerPopup"
                                    | "ColorPickerPopup"
                                    | "FontPickerPopup"
                                    | "Menu"
                                    | "MenuBar"
                                    | "ListView"
                                    | "GridView"
                                    | "TableView"
                                    | "StandardTableView"
                                    | "TreeView"
                            )
                        },
                    ) {
                        return fail(
                            content.span,
                            "Tooltip content cannot be a popup or virtual view",
                        );
                    }
                }
            }
            if table_view(kind) {
                validate_table(kind, members, span)?;
            }
            if kind == "SwipeDelegate" {
                validate_swipe_delegate(members, span)?;
            }
            if kind == "TreeView" {
                let mut children = members.iter().filter_map(|member| match member {
                    Member::Element(element) if !nonvisual_element(&element.ty) => Some(element),
                    _ => None,
                });
                if !children.next().is_some_and(|child| {
                    child.ty == "TreeRow" && child.repeat.is_some() && child.condition.is_none()
                }) || children.next().is_some()
                {
                    return fail(span, "TreeView requires exactly one repeated TreeRow");
                }
            }
            if kind == "ContextMenuArea"
                && members
                    .iter()
                    .filter(
                        |member| matches!(member, Member::Element(element) if element.ty == "Menu"),
                    )
                    .count()
                    != 1
            {
                return fail(span, "ContextMenuArea requires one direct Menu");
            }
            if matches!(kind, "ListView" | "GridView") {
                let mut elements = members.iter().filter_map(|member| match member {
                    Member::Element(element) if !nonvisual_element(&element.ty) => Some(element),
                    _ => None,
                });
                if !elements
                    .next()
                    .is_some_and(|element| element.repeat.is_some())
                    || elements.next().is_some()
                {
                    return fail(span, format!("{kind} requires exactly one for element"));
                }
            }
            if kind == "StackView"
                && !members
                    .iter()
                    .any(|member| matches!(member, Member::Element(e) if !nonvisual_element(&e.ty)))
            {
                return fail(span, "StackView requires at least one named StackPage");
            }
            let mut page_slots = BTreeSet::new();
            for (index, m) in members.iter().enumerate() {
                if let Member::Element(e) = m {
                    if nonvisual_element(&e.ty) {
                        if e.ty == "SecondaryWindow" && (owner != "root" || !is_window(kind)) {
                            return fail(e.span, "SecondaryWindow requires a root Window parent");
                        }
                        let Some(id) = e.id.as_deref().filter(|_| {
                            (owner == "root" || kind == "ActionGroup" && e.ty == "Action")
                                && e.repeat.is_none()
                                && e.condition.is_none()
                        }) else {
                            return fail(
                                e.span,
                                format!(
                                    "{} requires a named static declaration at the component root",
                                    e.ty
                                ),
                            );
                        };
                        check_name(id, e.span)?;
                        check_element_type(definitions, e)?;
                        if let Some(definition) = definitions.get(e.ty.as_str()) {
                            self.reference(e.type_span, "", 0, definition.span, &definition.name);
                        }
                        self.register(
                            definitions,
                            &e.members,
                            &e.ty,
                            id,
                            Some(owner),
                            e.span,
                            e.placement_scope.as_ref(),
                        )?;
                        continue;
                    }
                    if e.ty == "TreeRow" && kind != "TreeView" {
                        return fail(e.span, "TreeRow requires a TreeView parent");
                    }
                    if swipe_slot(&e.ty) && kind != "SwipeDelegate" {
                        return fail(e.span, "swipe slots require a direct SwipeDelegate parent");
                    }
                    if e.ty == "Tooltip" && e.repeat.is_some() {
                        return fail(e.span, "Tooltip cannot be a repeated child");
                    }
                    if matches!(e.ty.as_str(), "PageHeader" | "PageFooter")
                        || kind == "ApplicationWindow" && e.ty == "MenuBar"
                    {
                        if !matches!(kind, "Page" | "ApplicationWindow") || e.repeat.is_some() {
                            return fail(
                                e.span,
                                format!(
                                    "{} requires a direct Page parent or ApplicationWindow parent and cannot repeat",
                                    e.ty
                                ),
                            );
                        }
                        if !page_slots.insert(&e.ty) {
                            return fail(e.span, format!("{kind} accepts only one {}", e.ty));
                        }
                    }
                    if kind == "SplitView" && e.repeat.is_some() {
                        return fail(e.span, "SplitView panes require stable child positions");
                    }
                    if e.ty == "TableCell" && (kind != "TableRow" || e.repeat.is_some()) {
                        return fail(e.span, "TableCell must be a direct cell of TableRow");
                    }
                    if matches!(kind, "TableCell" | "TreeCell" | "StandardListView") {
                        return fail(e.span, format!("{kind} cannot contain child elements"));
                    }
                    if kind == "TimePicker" || picker_popup(kind) {
                        return fail(e.span, format!("{kind} uses built-in picker children"));
                    }
                    if kind == "DialogButtonBox"
                        && (!matches!(e.ty.as_str(), "Button" | "DialogButton" | "StandardButton")
                            || e.repeat.is_some())
                    {
                        return fail(
                            e.span,
                            "DialogButtonBox requires direct button children without repetition",
                        );
                    }
                    if matches!(
                        kind,
                        "RangeSlider"
                            | "Dial"
                            | "PageIndicator"
                            | "Calendar"
                            | "MonthGrid"
                            | "DayOfWeekRow"
                            | "WeekNumberColumn"
                            | "Tumbler"
                            | "Spinner"
                            | "BusyIndicator"
                    ) && e.ty != "Tooltip"
                        && !(kind == "MonthGrid" && e.ty == "DayCell")
                    {
                        return fail(e.span, format!("{kind} only accepts Tooltip children"));
                    }
                    if e.ty == "DayCell" {
                        if kind != "MonthGrid"
                            || e.id.is_some()
                            || e.repeat.is_some()
                            || e.condition.is_some()
                        {
                            return fail(
                                e.span,
                                "DayCell requires an anonymous template directly in MonthGrid",
                            );
                        }
                        if !page_slots.insert(&e.ty) {
                            return fail(e.span, "MonthGrid accepts only one DayCell");
                        }
                    }
                    if let Some(expected) = control_part_parent(&e.ty) {
                        if kind != expected || e.repeat.is_some() || e.condition.is_some() {
                            return fail(
                                e.span,
                                format!("{} requires a static child of {expected}", e.ty),
                            );
                        }
                        if !page_slots.insert(&e.ty) {
                            return fail(e.span, format!("{kind} accepts at most one {}", e.ty));
                        }
                    }
                    if e.ty == "SplitHandle" {
                        if kind != "SplitView"
                            || e.id.is_some()
                            || e.repeat.is_some()
                            || e.condition.is_some()
                        {
                            return fail(
                                e.span,
                                "SplitHandle requires an anonymous template directly in SplitView",
                            );
                        }
                        if !page_slots.insert(&e.ty) {
                            return fail(e.span, "SplitView accepts only one SplitHandle");
                        }
                    }
                    if (e.ty == "TableColumn" && !table_view(kind))
                        || (e.ty == "TableRow" && kind != "TableView")
                    {
                        return fail(
                            e.span,
                            format!("{} must be a direct child of TableView", e.ty),
                        );
                    }
                    if matches!(kind, "MenuItem" | "MenuSeparator")
                        || kind == "MenuBar" && e.ty != "Menu"
                        || kind == "Menu"
                            && !matches!(e.ty.as_str(), "Menu" | "MenuItem" | "MenuSeparator")
                        || e.ty == "Menu"
                            && !matches!(
                                kind,
                                "MenuBar" | "Menu" | "ContextMenuArea" | "SystemTrayIcon"
                            )
                        || matches!(e.ty.as_str(), "MenuItem" | "MenuSeparator") && kind != "Menu"
                    {
                        return fail(e.span, format!("{} is not a valid child of {kind}", e.ty));
                    }
                    let row_delegate = !is_builtin(&e.ty) && {
                        let mut ancestor = Some(owner);
                        let mut found = false;
                        while let Some(key) = ancestor {
                            let element = &self.owners[key];
                            if matches!(element.kind.as_str(), "TableRow" | "TreeRow") {
                                found = true;
                                break;
                            }
                            ancestor = element.parent.as_deref();
                        }
                        found
                    };
                    if !is_builtin(&e.ty) && (e.repeat.is_some() || row_delegate) {
                        if e.repeat.is_some()
                            && (!matches!(kind, "ListView" | "GridView") || e.id.is_some())
                        {
                            return fail(
                                e.span,
                                "repeated component instances require an anonymous ListView template or an anonymous GridView template",
                            );
                        }
                        if row_delegate
                            && contains_element(
                                &e.ty,
                                &e.members,
                                definitions,
                                &mut BTreeSet::new(),
                                |kind| is_window(kind) || kind == "SecondaryWindow",
                            )
                        {
                            return fail(
                                e.span,
                                format!("{kind} delegates cannot contain a window"),
                            );
                        }
                    }
                    let required = match kind {
                        "RadioGroup" => Some("RadioButton"),
                        "TabWidget" => Some("Tab"),
                        "TabBar" => Some("TabButton"),
                        "SwipeView" => Some("SwipePage"),
                        "StackView" => Some("StackPage"),
                        _ => None,
                    };
                    if let Some(required) = required
                        && (delegate_base(&e.ty).unwrap_or(&e.ty) != required
                            || e.condition.is_some()
                            || (matches!(kind, "TabWidget" | "SwipeView" | "StackView")
                                && e.repeat.is_some()))
                    {
                        return fail(
                            e.span,
                            format!("{kind} requires static {required} children"),
                        );
                    }
                    if kind == "StackView" && e.id.is_none() {
                        return fail(e.span, "StackPage requires a route ID");
                    }
                    if matches!(kind, "RadioGroup" | "TabBar")
                        && e.repeat.is_some()
                        && members
                            .iter()
                            .filter(|member| matches!(member, Member::Element(_)))
                            .count()
                            != 1
                    {
                        return fail(
                            e.span,
                            format!("a repeated {} must be the only option", e.ty),
                        );
                    }
                    if matches!(kind, "RadioButton" | "TabButton") {
                        return fail(e.span, format!("{kind} cannot contain child elements"));
                    }
                    let virtual_radio = e.ty == "RadioDelegate"
                        && e.repeat.is_some()
                        && matches!(kind, "ListView" | "GridView");
                    if (delegate_base(&e.ty).unwrap_or(&e.ty) == "RadioButton" && kind != "RadioGroup"
                        && !virtual_radio
                        && !e.members.iter().any(|member| matches!(member, Member::Binding(binding) if binding.name == "group")))
                        || (e.ty == "Tab" && kind != "TabWidget")
                        || (e.ty == "TabButton" && kind != "TabBar")
                        || (e.ty == "SwipePage" && kind != "SwipeView")
                        || (e.ty == "StackPage" && kind != "StackView")
                    {
                        return fail(
                            e.span,
                            format!("{} requires a matching selection container", e.ty),
                        );
                    }
                    if e.ty == "Row" && kind != "GridLayout" {
                        return fail(e.span, "Row must be a direct child of GridLayout");
                    }
                    if let Some(id) = &e.id {
                        check_name(id, e.span)?;
                    }
                    let child = key(e, owner, index);
                    check_element_type(definitions, e)?;
                    let source_type = e
                        .placement_scope
                        .as_ref()
                        .and_then(|placement| placement.source_component.as_deref())
                        .unwrap_or(&e.ty);
                    if let Some(definition) = definitions.get(source_type) {
                        self.reference(e.type_span, "", 0, definition.span, &definition.name);
                    }
                    self.register(
                        definitions,
                        &e.members,
                        &e.ty,
                        &child,
                        Some(owner),
                        e.span,
                        e.placement_scope.as_ref(),
                    )?;
                    self.owners
                        .get_mut(&child)
                        .expect("registered child")
                        .repeated = e.repeat.is_some();
                }
            }
        }
        Ok(())
    }

    fn lexical_scope(&self, owner: &str) -> usize {
        self.owners[owner]
            .placement
            .map_or(0, |placement| placement.lexical_scope)
    }

    fn receiver<'b>(&'b self, current: &'b str, prefix: &'b str) -> Option<&'b str> {
        if prefix == "self" {
            return Some(current);
        }
        let (source, global_name) = prefix.split_once('@').unwrap_or((prefix, prefix));
        let prefix = source;
        let scope = self.caller_scope.as_ref().map_or_else(
            || self.lexical_scope(current),
            |(placement, _)| placement.caller.as_ref().unwrap().0,
        );
        let names = self.placement_names.get(scope);
        if prefix == "root" {
            return Some(names.map_or("root", |names| names.root.as_str()));
        }
        if let Some(key) = names.and_then(|names| names.ids.get(prefix)) {
            return Some(key);
        }
        if scope == 0
            && self.owners.get(prefix).is_some_and(|target| {
                target.placement.is_none_or(|placement| {
                    placement
                        .caller
                        .as_ref()
                        .map_or(placement.lexical_scope, |caller| caller.0)
                        == 0
                })
            })
        {
            return Some(prefix);
        }
        if self.global && global_name == self.component_name {
            return Some("root");
        }
        self.owners
            .get_key_value(&format!("__global_{global_name}"))
            .map(|(key, _)| key.as_str())
    }

    fn lexical_parent<'b>(&'b self, owner: &'b str) -> Option<&'b str> {
        let element = &self.owners[owner];
        if let Some((placement, root)) = &self.caller_scope
            && root == owner
        {
            return Some(&placement.caller.as_ref().unwrap().1);
        }
        match element.placement {
            Some(placement) => placement.lexical_parent.as_deref(),
            None => element.parent.as_deref(),
        }
    }

    fn for_caller(&self, owner: &str, member: &str) -> Option<Self> {
        let placement = self.owners[owner].placement?;
        if !placement.caller_members.contains(member) {
            return None;
        }
        let mut scope = self.clone();
        scope.caller_scope = Some((placement, owner.into()));
        Some(scope)
    }

    fn member_owner<'b>(
        &'b self,
        current: &'b str,
        reference: &'b str,
        kind: &str,
        span: Span,
        contains: impl Fn(&Owner<'a>, &str) -> bool,
    ) -> Result<(&'b str, &'b str)> {
        if let Some((prefix, name)) = reference.split_once('.') {
            let owner = self.receiver(current, prefix).ok_or_else(|| Diagnostic {
                span,
                message: format!("unknown element id `{prefix}`"),
            })?;
            Ok((owner, name))
        } else {
            let mut owner = current;
            loop {
                let scope = &self.owners[owner];
                if contains(scope, reference) {
                    return Ok((owner, reference));
                }
                owner = self.lexical_parent(owner).ok_or_else(|| Diagnostic {
                    span,
                    message: format!("unknown {kind} `{reference}`"),
                })?;
            }
        }
    }

    fn property(
        &self,
        current: &str,
        reference: &str,
        write: bool,
        span: Span,
    ) -> Result<(ir::PropertyRef, Type)> {
        if write
            && (self.stack_read(current, reference, span).is_some()
                || self.table_sort_read(current, reference, span)?.is_some()
                || self.table_history_read(current, reference, span)?.is_some()
                || self
                    .table_selection_read(current, reference, span)?
                    .is_some()
                || self.window_read(current, reference, span).is_some()
                || self.check_group_read(reference, span)?.is_some()
                || self.action_group_read(current, reference, span)?.is_some()
                || self.native_dialog_read(current, reference, span).is_some()
                || self.image_playback_read(current, reference, span).is_some()
                || self.media_read(current, reference, span).is_some()
                || self.viewport_read(current, reference, span)?.is_some()
                || self
                    .scroll_control_read(current, reference, span)?
                    .is_some()
                || self.geometry_read(current, reference, span)?.is_some()
                || self.gesture_read(current, reference, span)?.is_some()
                || self.split_resizing(current, reference, span)?.is_some()
                || self.focus_read(current, reference, span)?.is_some())
        {
            return fail(span, format!("property `{reference}` is read-only"));
        }
        let (owner, name) =
            self.member_owner(current, reference, "property", span, |scope, name| {
                scope.properties.contains_key(name)
            })?;
        let scope = &self.owners[owner];
        let p = scope.properties.get(name).ok_or_else(|| Diagnostic {
            span,
            message: format!("unknown property `{reference}`"),
        })?;
        let external = scope.instance
            || scope.placement.is_some_and(|placement| {
                placement.caller.is_some()
                    && self.caller_scope.as_ref().map_or_else(
                        || self.lexical_scope(current),
                        |(caller, _)| caller.caller.as_ref().unwrap().0,
                    ) != placement.lexical_scope
            });
        if external && p.visibility == "private" {
            return fail(span, format!("property `{reference}` is private"));
        }
        if write && ((external && p.visibility == "out") || (!external && p.visibility == "in")) {
            return fail(
                span,
                format!("property `{reference}` is read-only in this scope"),
            );
        }
        self.member_reference(span, reference, owner, p.span, &p.name);
        Ok((
            ir::PropertyRef {
                owner: owner.into(),
                name: name.into(),
            },
            p.ty.clone(),
        ))
    }

    fn check_group_read(&self, reference: &str, span: Span) -> Result<Option<ir::Expr>> {
        let Some((owner, "check-state")) = reference.split_once('.') else {
            return Ok(None);
        };
        let Some(group) = self
            .owners
            .get(owner)
            .filter(|group| group.kind == "ButtonGroup")
        else {
            return Ok(None);
        };
        if !non_exclusive_group(group.members) {
            return fail(span, "ButtonGroup check-state requires exclusive: false");
        }
        Ok(Some(ir::Expr {
            value: "check-state".into(),
            owner: Some(owner.into()),
            ..ir::Expr::operation("check_group_read", Type::CheckState, vec![], span)
        }))
    }

    fn action_group_read(
        &self,
        current: &str,
        reference: &str,
        span: Span,
    ) -> Result<Option<ir::Expr>> {
        let Some((owner, "current-value")) = reference.split_once('.') else {
            return Ok(None);
        };
        let owner = self.receiver(current, owner).unwrap_or(owner);
        let Some(group) = self
            .owners
            .get(owner)
            .filter(|group| group.kind == "ActionGroup")
        else {
            return Ok(None);
        };
        if non_exclusive_group(group.members) {
            return fail(
                span,
                "ActionGroup current-value requires an exclusive group",
            );
        }
        Ok(Some(ir::Expr {
            op: "action_group_read".into(),
            owner: Some(owner.into()),
            value: "current-value".into(),
            ty: Type::String,
            args: vec![],
            span,
        }))
    }

    fn window_read(&self, current: &str, reference: &str, span: Span) -> Option<ir::Expr> {
        let (owner, name) = self
            .member_owner(current, reference, "property", span, |element, name| {
                element.properties.contains_key(name)
                    || is_window(&element.kind) && window_status_property(name)
            })
            .ok()?;
        (owner == "root" && is_window(&self.owners[owner].kind) && window_status_property(name))
            .then(|| ir::Expr {
                value: name.into(),
                ..ir::Expr::operation("window_read", Type::Bool, vec![], span)
            })
    }

    fn native_dialog_read(&self, current: &str, reference: &str, span: Span) -> Option<ir::Expr> {
        let (owner, property) = reference.split_once('.')?;
        let owner = self.receiver(current, owner).unwrap_or(owner);
        let ty = native_dialog_property(&self.owners.get(owner)?.kind, property)?;
        Some(ir::Expr {
            owner: Some(owner.into()),
            value: property.into(),
            ..ir::Expr::operation("native_dialog_read", ty, vec![], span)
        })
    }

    fn focus_read(&self, current: &str, reference: &str, span: Span) -> Result<Option<ir::Expr>> {
        let Ok((owner, property)) =
            self.member_owner(current, reference, "property", span, |element, name| {
                element.properties.contains_key(name) || focus_property(&element.kind, name)
            })
        else {
            return Ok(None);
        };
        let mut element = &self.owners[owner];
        if element.instance || !focus_property(&element.kind, property) {
            return Ok(None);
        }
        loop {
            if element.repeated {
                return fail(
                    span,
                    "focus bindings on repeated elements require a component delegate",
                );
            }
            let Some(parent) = &element.parent else {
                break;
            };
            element = &self.owners[parent];
        }
        Ok(Some(ir::Expr {
            op: "focus_read".into(),
            owner: Some(owner.into()),
            value: property.into(),
            ty: Type::Bool,
            args: Vec::new(),
            span,
        }))
    }

    fn require_static_viewport(&self, owner: &str, span: Span) -> Result<()> {
        let mut current = Some(owner);
        while let Some(key) = current {
            let element = &self.owners[key];
            if element.repeated {
                return fail(
                    span,
                    "viewport access on repeated elements requires a component delegate",
                );
            }
            current = element.parent.as_deref();
        }
        Ok(())
    }

    fn gesture_read(&self, current: &str, reference: &str, span: Span) -> Result<Option<ir::Expr>> {
        let Some((id, name)) = reference.split_once('.') else {
            return Ok(None);
        };
        let owner = self.receiver(current, id).unwrap_or(id);
        let Some(element) = self.owners.get(owner).filter(|element| !element.instance) else {
            return Ok(None);
        };
        let metric = scale_rotate_property(&element.kind, name)
            .map(|ty| ("scale_rotate_read", ty))
            .or_else(|| swipe_property(&element.kind, name).map(|ty| ("swipe_read", ty)))
            .or_else(|| hover_property(&element.kind, name).map(|ty| ("hover_read", ty)))
            .or_else(|| touch_area_property(&element.kind, name).map(|ty| ("touch_area_read", ty)));
        let Some((op, ty)) = metric else {
            return Ok(None);
        };
        let mut ancestor = Some(owner);
        while let Some(key) = ancestor {
            let element = &self.owners[key];
            if element.repeated {
                return fail(
                    span,
                    "gesture reads in repeated elements require a component delegate",
                );
            }
            ancestor = element.parent.as_deref();
        }
        Ok(Some(ir::Expr {
            op: op.into(),
            owner: Some(owner.into()),
            value: name.into(),
            ty,
            args: vec![],
            span,
        }))
    }

    fn scroll_control_read(
        &self,
        current: &str,
        reference: &str,
        span: Span,
    ) -> Result<Option<ir::Expr>> {
        let Some((id, name @ ("position" | "size"))) = reference.split_once('.') else {
            return Ok(None);
        };
        let owner = self.receiver(current, id).unwrap_or(id);
        if !self
            .owners
            .get(owner)
            .is_some_and(|element| !element.instance && scroll_control(&element.kind))
        {
            return Ok(None);
        }
        self.require_static_viewport(owner, span)?;
        Ok(Some(ir::Expr {
            op: "scroll_control_read".into(),
            owner: Some(owner.into()),
            value: name.into(),
            ty: Type::Float,
            args: vec![],
            span,
        }))
    }

    fn viewport_read(
        &self,
        current: &str,
        reference: &str,
        span: Span,
    ) -> Result<Option<ir::Expr>> {
        let Ok((owner, property)) =
            self.member_owner(current, reference, "property", span, |element, name| {
                element.properties.contains_key(name)
                    || viewport_property(&element.kind, name)
                    || flick_motion_property(&element.kind, name).is_some()
            })
        else {
            return Ok(None);
        };
        let element = &self.owners[owner];
        let motion = flick_motion_property(&element.kind, property);
        if element.instance || !(motion.is_some() || viewport_property(&element.kind, property)) {
            return Ok(None);
        }
        self.require_static_viewport(owner, span)?;
        Ok(Some(ir::Expr {
            op: if motion.is_some() {
                "flick_motion_read"
            } else {
                "viewport_read"
            }
            .into(),
            owner: Some(owner.into()),
            value: property.into(),
            ty: motion.unwrap_or(Type::Length),
            args: Vec::new(),
            span,
        }))
    }

    fn geometry_read(
        &self,
        current: &str,
        reference: &str,
        span: Span,
    ) -> Result<Option<ir::Expr>> {
        let Ok((owner, name)) =
            self.member_owner(current, reference, "property", span, |scope, name| {
                scope.properties.contains_key(name)
                    || geometry_property_for(&scope.kind, name).is_some()
            })
        else {
            return Ok(None);
        };
        let element = &self.owners[owner];
        let Some(ty) = geometry_property(name) else {
            return Ok(None);
        };
        if matches!(name, "available-width" | "available-height")
            && !content_geometry_owner(&element.kind)
        {
            return Ok(None);
        }
        if !geometry_owner(&element.kind) {
            return fail(span, "layout geometry requires a visual target");
        }
        let mut ancestor = Some(owner);
        while let Some(key) = ancestor {
            let target = &self.owners[key];
            if target.repeated {
                return fail(
                    span,
                    "geometry reads on repeated elements require a component delegate",
                );
            }
            ancestor = target.parent.as_deref();
        }
        Ok(Some(ir::Expr {
            op: "geometry_read".into(),
            owner: Some(owner.into()),
            value: name.into(),
            ty,
            args: vec![],
            span,
        }))
    }

    fn table_selection_read(
        &self,
        current: &str,
        reference: &str,
        span: Span,
    ) -> Result<Option<ir::Expr>> {
        let Some((receiver, name)) = reference.rsplit_once('.') else {
            return Ok(None);
        };
        let owner = self.receiver(current, receiver).unwrap_or(receiver);
        let Some(element) = self.owners.get(owner).filter(|element| !element.instance) else {
            return Ok(None);
        };
        let Some(ty) = table_selection_property(&element.kind, name) else {
            return Ok(None);
        };
        self.require_static_viewport(owner, span)?;
        Ok(Some(ir::Expr {
            op: "table_selection_read".into(),
            owner: Some(owner.into()),
            value: name.into(),
            ty,
            args: Vec::new(),
            span,
        }))
    }

    fn table_sort_read(
        &self,
        current: &str,
        reference: &str,
        span: Span,
    ) -> Result<Option<ir::Expr>> {
        let Some((receiver, name)) = reference.rsplit_once('.') else {
            return Ok(None);
        };
        let owner = self.receiver(current, receiver).unwrap_or(receiver);
        let Some(element) = self.owners.get(owner).filter(|element| !element.instance) else {
            return Ok(None);
        };
        let Some(ty) = table_sort_property(&element.kind, name) else {
            return Ok(None);
        };
        self.require_static_viewport(owner, span)?;
        Ok(Some(ir::Expr {
            op: "table_sort_read".into(),
            owner: Some(owner.into()),
            value: name.into(),
            ty,
            args: Vec::new(),
            span,
        }))
    }

    fn table_history_read(
        &self,
        current: &str,
        reference: &str,
        span: Span,
    ) -> Result<Option<ir::Expr>> {
        let Some((receiver, name)) = reference.rsplit_once('.') else {
            return Ok(None);
        };
        let owner = self.receiver(current, receiver).unwrap_or(receiver);
        let Some(element) = self.owners.get(owner).filter(|element| !element.instance) else {
            return Ok(None);
        };
        let Some(ty) = table_history_property(&element.kind, name) else {
            return Ok(None);
        };
        self.require_static_viewport(owner, span)?;
        Ok(Some(ir::Expr {
            op: "table_history_read".into(),
            owner: Some(owner.into()),
            value: name.into(),
            ty,
            args: Vec::new(),
            span,
        }))
    }

    fn stack_read(&self, current: &str, reference: &str, span: Span) -> Option<ir::Expr> {
        let (receiver, name) = reference.rsplit_once('.')?;
        let owner = self.receiver(current, receiver).unwrap_or(receiver);
        let element = self.owners.get(owner)?;
        let (stack, ty, args) = match element.kind.as_str() {
            "StackView" => (owner, stack_read_type(name)?, Vec::new()),
            "StackPage" => (
                element.parent.as_deref()?,
                stack_page_read_type(name)?,
                vec![ir::Expr::literal(
                    Type::String,
                    serde_json::to_string(owner).unwrap(),
                    span,
                )],
            ),
            _ => return None,
        };
        Some(ir::Expr {
            op: "stack_read".into(),
            owner: Some(stack.into()),
            value: name.into(),
            ty,
            args,
            span,
        })
    }

    fn media_read(&self, current: &str, reference: &str, span: Span) -> Option<ir::Expr> {
        let (receiver, name) = reference.rsplit_once('.')?;
        let owner = self.receiver(current, receiver).unwrap_or(receiver);
        if self.owners.get(owner)?.kind != "MediaPlayer" {
            return None;
        }
        Some(ir::Expr {
            op: "media_read".into(),
            owner: Some(owner.into()),
            value: name.into(),
            ty: media_read_type(name)?,
            args: Vec::new(),
            span,
        })
    }

    fn image_playback_read(&self, current: &str, reference: &str, span: Span) -> Option<ir::Expr> {
        let (receiver, name) = reference.rsplit_once('.')?;
        let owner = self.receiver(current, receiver).unwrap_or(receiver);
        if self.owners.get(owner)?.kind != "AnimatedImage" || !playback_property(name) {
            return None;
        }
        Some(ir::Expr {
            op: "image_playback_read".into(),
            owner: Some(owner.into()),
            value: name.into(),
            ty: Type::Int,
            args: Vec::new(),
            span,
        })
    }

    fn call(&self, call: &ast::Call, current: &str) -> Result<(ir::CallbackCall, Option<Type>)> {
        let span = call.span;
        let reference = &call.callee;
        if !reference.contains('.') && self.parameters.contains_key(reference) {
            return fail(span, format!("parameter `{reference}` cannot be called"));
        }
        let (owner, name) =
            self.member_owner(current, reference, "callback", span, |scope, name| {
                scope.callbacks.contains_key(name) || scope.properties.contains_key(name)
            })?;
        let callback = self
            .callback_info(owner, name)
            .map(|info| &info.signature)
            .or_else(|| self.owners[owner].callbacks.get(name).copied())
            .ok_or_else(|| Diagnostic {
                span,
                message: format!("unknown callback `{reference}`"),
            })?;
        if let Some(declaration) = self.owners[owner].callbacks.get(name) {
            self.member_reference(span, reference, owner, declaration.span, name);
        }
        if !callback.pure && !self.effects_allowed {
            return fail(
                span,
                format!("impure callback `{reference}` cannot be called in a pure context"),
            );
        }
        if call.arguments.len() != callback.parameters.len() {
            return fail(
                span,
                format!(
                    "callback `{reference}` expects {} arguments, found {}",
                    callback.parameters.len(),
                    call.arguments.len()
                ),
            );
        }
        let args = call
            .arguments
            .iter()
            .zip(&callback.parameters)
            .map(|(value, param)| expression_as(value, self, current, param.ty.clone()))
            .collect::<Result<Vec<_>>>()?;
        Ok((
            ir::CallbackCall {
                owner: owner.into(),
                name: name.into(),
                args,
                span,
            },
            callback.return_type.clone(),
        ))
    }

    fn check_stack_route(&self, owner: &str, page: &ir::Expr) -> Result<()> {
        if page.op == "literal"
            && let Ok(route) = serde_json::from_str::<String>(&page.value)
            && !self.owners.get(&route).is_some_and(|page| {
                page.kind == "StackPage" && page.parent.as_deref() == Some(owner)
            })
        {
            return fail(
                page.span,
                format!("unknown StackPage route `{route}` for `{owner}`"),
            );
        }
        Ok(())
    }

    fn check_stack_path(&self, owner: &str, path: &ir::Expr) -> Result<()> {
        if path.op != "array" {
            return Ok(());
        }
        if path.args.is_empty() {
            return fail(path.span, "StackView path requires at least one route");
        }
        let mut routes = BTreeSet::new();
        for page in &path.args {
            self.check_stack_route(owner, page)?;
            if page.op == "literal"
                && let Ok(route) = serde_json::from_str::<String>(&page.value)
                && !routes.insert(route.clone())
            {
                return fail(page.span, format!("duplicate StackPage route `{route}`"));
            }
        }
        Ok(())
    }

    fn split_resizing(
        &self,
        current: &str,
        reference: &str,
        span: Span,
    ) -> Result<Option<ir::Expr>> {
        let Some((id, name @ "resizing")) = reference.split_once('.') else {
            return Ok(None);
        };
        let owner = self.receiver(current, id).unwrap_or(id);
        if !self
            .owners
            .get(owner)
            .is_some_and(|element| !element.instance && element.kind == "SplitView")
        {
            return Ok(None);
        }
        self.require_static_viewport(owner, span)?;
        Ok(Some(ir::Expr {
            op: "split_resizing".into(),
            owner: Some(owner.into()),
            value: name.into(),
            ty: Type::Bool,
            args: vec![],
            span,
        }))
    }

    fn split_state_read(&self, call: &ast::Call, current: &str) -> Result<Option<ir::Expr>> {
        let Some((receiver, "save-state")) = call.callee.rsplit_once('.') else {
            return Ok(None);
        };
        let owner = self.receiver(current, receiver).unwrap_or(receiver);
        if !self.owners.get(owner).is_some_and(|target| {
            !target.instance
                && target.kind == "SplitView"
                && !target.callbacks.contains_key("save-state")
        }) {
            return Ok(None);
        }
        self.require_static_viewport(owner, call.span)?;
        if !call.arguments.is_empty() {
            return fail(call.span, "save-state() takes no arguments");
        }
        Ok(Some(ir::Expr {
            op: "split_state_read".into(),
            owner: Some(owner.into()),
            value: String::new(),
            ty: Type::String,
            args: Vec::new(),
            span: call.span,
        }))
    }

    fn tree_query(&self, call: &ast::Call, current: &str) -> Result<Option<ir::Expr>> {
        let Some((receiver, method)) = call.callee.rsplit_once('.') else {
            return Ok(None);
        };
        let owner = self.receiver(current, receiver).unwrap_or(receiver);
        if !matches!(method, "is-expanded" | "depth" | "row-for-key")
            || !self.owners.get(owner).is_some_and(|target| {
                !target.instance
                    && target.kind == "TreeView"
                    && !target.callbacks.contains_key(method)
            })
        {
            return Ok(None);
        }
        self.require_static_viewport(owner, call.span)?;
        if call.arguments.len() != 1 {
            return fail(call.span, format!("{method}() takes one key argument"));
        }
        let key = expression_as(&call.arguments[0], self, current, Type::Int)?;
        Ok(Some(ir::Expr {
            op: "tree_query".into(),
            owner: Some(owner.into()),
            value: method.into(),
            ty: if method == "is-expanded" {
                Type::Bool
            } else {
                Type::Int
            },
            args: vec![key],
            span: call.span,
        }))
    }

    fn view_index_query(&self, call: &ast::Call, current: &str) -> Result<Option<ir::Expr>> {
        let Some((receiver, method)) = call.callee.rsplit_once('.') else {
            return Ok(None);
        };
        let owner = self.receiver(current, receiver).unwrap_or(receiver);
        if !matches!(method, "view-index-for-source" | "source-index-for-view")
            || !self.owners.get(owner).is_some_and(|target| {
                !target.instance
                    && !target.callbacks.contains_key(method)
                    && viewport_kind(&target.kind)
                    && !content_viewport(&target.kind)
            })
        {
            return Ok(None);
        }
        self.require_static_viewport(owner, call.span)?;
        let [index] = call.arguments.as_slice() else {
            return fail(call.span, format!("{method}() takes one index argument"));
        };
        Ok(Some(ir::Expr {
            op: "view_index".into(),
            owner: Some(owner.into()),
            value: method.into(),
            ty: Type::Int,
            args: vec![expression_as(index, self, current, Type::Int)?],
            span: call.span,
        }))
    }

    fn builtin_call(&self, call: &ast::Call) -> Result<Option<&'static builtins::Function>> {
        let Some((namespace, member)) = call.callee.split_once('.') else {
            return Ok(None);
        };
        if self.owners.contains_key(namespace)
            || self.parameters.contains_key(namespace)
            || !builtins::NAMESPACES.contains(&namespace)
        {
            return Ok(None);
        }
        builtins::function(namespace, member)
            .map(Some)
            .ok_or_else(|| Diagnostic {
                span: call.span,
                message: format!("unknown built-in function `{}`", call.callee),
            })
    }

    fn log_call(
        &self,
        call: &ast::Call,
        current: &str,
        kind: FunctionKind,
    ) -> Result<ir::Statement> {
        let name = call.callee.as_str();
        let (level, message) = match (kind, call.arguments.as_slice()) {
            (FunctionKind::Log, [level, message]) => (
                expression_as(level, self, current, Type::LogLevel)?,
                message,
            ),
            (FunctionKind::LogAt(level), [message]) => {
                (ir::Expr::literal(Type::LogLevel, level, call.span), message)
            }
            (FunctionKind::Log, _) => {
                return fail(call.span, "Log.write expects a level and a message");
            }
            (FunctionKind::LogAt(_), _) => {
                return fail(call.span, format!("{name} expects one message"));
            }
            _ => {
                return fail(
                    call.span,
                    format!("{name} produces a value and must be used as an expression"),
                );
            }
        };
        if !self.effects_allowed {
            return fail(
                call.span,
                format!("{name} is not allowed in a pure callback"),
            );
        }
        Ok(ir::Statement::Log {
            level,
            message: expression_as(message, self, current, Type::String)?,
            span: call.span,
        })
    }

    fn effect_call(&self, call: &ast::Call, current: &str) -> Result<ir::Statement> {
        if let Some(function) = self.builtin_call(call)? {
            return self.log_call(call, current, function.kind);
        }
        if let Some((receiver, method)) = call.callee.rsplit_once('.') {
            let owner = self.receiver(current, receiver).unwrap_or(receiver);
            if matches!(method, "select-source-index" | "select-source-cell")
                && let Some(target) = self.owners.get(owner)
                && !target.instance
                && !target.callbacks.contains_key(method)
            {
                if !viewport_kind(&target.kind) || content_viewport(&target.kind) {
                    return fail(call.span, format!("{method}() requires a virtual view"));
                }
                let cell = method == "select-source-cell";
                if cell && !table_view(&target.kind) {
                    return fail(call.span, "select-source-cell() requires a table");
                }
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "source selection is not allowed in a pure callback",
                    );
                }
                self.require_static_viewport(owner, call.span)?;
                let arity = if cell { 2 } else { 1 };
                if call.arguments.len() != arity {
                    return fail(call.span, format!("{method}() expects {arity} arguments"));
                }
                let args = call
                    .arguments
                    .iter()
                    .map(|argument| expression_as(argument, self, current, Type::Int))
                    .collect::<Result<Vec<_>>>()?;
                return Ok(ir::Statement::SourceSelection(ir::SourceSelection {
                    source_view: owner.into(),
                    method: method.into(),
                    args,
                    span: call.span,
                }));
            }
            if matches!(
                method,
                "select-cell"
                    | "set-current-cell"
                    | "toggle-cell"
                    | "select-row"
                    | "select-column"
                    | "select-range"
                    | "select-all"
                    | "clear-selection"
            ) && self.owners.get(owner).is_some_and(|target| {
                !target.instance
                    && table_view(&target.kind)
                    && !target.callbacks.contains_key(method)
            }) {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "table selection commands are not allowed in a pure callback",
                    );
                }
                self.require_static_viewport(owner, call.span)?;
                let arity = match method {
                    "select-all" | "clear-selection" => 0,
                    "select-cell" | "set-current-cell" | "toggle-cell" => 2,
                    "select-range" => 4,
                    _ => 1,
                };
                if call.arguments.len() != arity {
                    return fail(call.span, format!("{method}() expects {arity} arguments"));
                }
                let args = call
                    .arguments
                    .iter()
                    .map(|argument| expression_as(argument, self, current, Type::Int))
                    .collect::<Result<Vec<_>>>()?;
                return Ok(ir::Statement::TableSelectionCommand(
                    ir::TableSelectionCommand {
                        selection_owner: owner.into(),
                        method: method.into(),
                        args,
                        span: call.span,
                    },
                ));
            }
            if matches!(method, "sort-by" | "clear-sort")
                && self.owners.get(owner).is_some_and(|target| {
                    !target.instance
                        && table_view(&target.kind)
                        && !target.callbacks.contains_key(method)
                })
            {
                if !self.effects_allowed {
                    return fail(call.span, "table sorting is not allowed in a pure callback");
                }
                self.require_static_viewport(owner, call.span)?;
                let types: &[Type] = if method == "sort-by" {
                    &[Type::Int, Type::Bool]
                } else {
                    &[]
                };
                if call.arguments.len() != types.len() {
                    return fail(
                        call.span,
                        format!("{method}() expects {} arguments", types.len()),
                    );
                }
                let args = call
                    .arguments
                    .iter()
                    .zip(types)
                    .map(|(argument, ty)| expression_as(argument, self, current, ty.clone()))
                    .collect::<Result<Vec<_>>>()?;
                return Ok(ir::Statement::TableSortCommand(ir::TableSortCommand {
                    sort_owner: owner.into(),
                    method: method.into(),
                    args,
                    span: call.span,
                }));
            }
            if matches!(method, "undo" | "redo")
                && self.owners.get(owner).is_some_and(|target| {
                    !target.instance
                        && table_view(&target.kind)
                        && !target.callbacks.contains_key(method)
                })
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "table history commands are not allowed in a pure callback",
                    );
                }
                self.require_static_viewport(owner, call.span)?;
                if !call.arguments.is_empty() {
                    return fail(call.span, format!("{method}() takes no arguments"));
                }
                return Ok(ir::Statement::TableHistoryCommand {
                    history_owner: owner.into(),
                    redo: method == "redo",
                    span: call.span,
                });
            }
            if method == "restore-state"
                && self.owners.get(owner).is_some_and(|target| {
                    !target.instance
                        && target.kind == "SplitView"
                        && !target.callbacks.contains_key(method)
                })
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "restoring split state is not allowed in a pure callback",
                    );
                }
                self.require_static_viewport(owner, call.span)?;
                let [state] = call.arguments.as_slice() else {
                    return fail(call.span, "restore-state() takes one string argument");
                };
                return Ok(ir::Statement::RestoreSplit(ir::RestoreSplit {
                    split_owner: owner.into(),
                    state: expression_as(state, self, current, Type::String)?,
                    span: call.span,
                }));
            }
            if matches!(
                method,
                "expand"
                    | "collapse"
                    | "toggle-expanded"
                    | "expand-recursively"
                    | "collapse-recursively"
                    | "expand-to"
            ) && self.owners.get(owner).is_some_and(|target| {
                !target.instance
                    && target.kind == "TreeView"
                    && !target.callbacks.contains_key(method)
            }) {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "tree commands are not allowed in a pure callback",
                    );
                }
                self.require_static_viewport(owner, call.span)?;
                let arity = match method {
                    "expand-recursively" => 0..=2,
                    "collapse-recursively" => 0..=1,
                    _ => 1..=1,
                };
                if !arity.contains(&call.arguments.len()) {
                    return fail(
                        call.span,
                        format!("incorrect number of arguments for tree command `{method}`"),
                    );
                }
                let mut args = call
                    .arguments
                    .iter()
                    .map(|argument| expression_as(argument, self, current, Type::Int))
                    .collect::<Result<Vec<_>>>()?;
                let operands = if method == "expand-recursively" { 2 } else { 1 };
                args.resize_with(operands, || ir::Expr::literal(Type::Int, "-1", call.span));
                return Ok(ir::Statement::TreeCommand(ir::TreeCommand {
                    tree_owner: owner.into(),
                    method: method.into(),
                    args,
                    span: call.span,
                }));
            }
            if method == "set-check-state"
                && let Some(group) = self
                    .owners
                    .get(owner)
                    .filter(|group| group.kind == "ButtonGroup")
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "group commands are not allowed in a pure callback",
                    );
                }
                if !non_exclusive_group(group.members) {
                    return fail(
                        call.span,
                        "set-check-state() requires a non-exclusive ButtonGroup",
                    );
                }
                let [value] = call.arguments.as_slice() else {
                    return fail(call.span, "set-check-state() takes one CheckState argument");
                };
                return Ok(ir::Statement::CheckGroupCommand(ir::CheckGroupCommand {
                    check_group: owner.into(),
                    state: expression_as(value, self, current, Type::CheckState)?,
                    span: call.span,
                }));
            }
            if matches!(method, "move-column" | "move-row" | "set-row-height")
                && let Some(target) = self.owners.get(owner)
                && !target.instance
                && table_view(&target.kind)
                && !target.callbacks.contains_key(method)
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        format!("{method}() is not allowed in a pure callback"),
                    );
                }
                let [first, second] = call.arguments.as_slice() else {
                    return fail(call.span, format!("{method}() takes two arguments"));
                };
                self.require_static_viewport(owner, call.span)?;
                let setting_height = method == "set-row-height";
                let values = [
                    expression_as(first, self, current, Type::Int)?,
                    expression_as(
                        second,
                        self,
                        current,
                        if setting_height {
                            Type::Length
                        } else {
                            Type::Int
                        },
                    )?,
                ];
                if setting_height {
                    return Ok(ir::Statement::SetRowHeight(ir::SetRowHeight {
                        row_height_owner: owner.into(),
                        arguments: values,
                        span: call.span,
                    }));
                }
                let move_item = ir::MoveTableItem {
                    table_owner: owner.into(),
                    positions: values,
                    span: call.span,
                };
                return Ok(if method == "move-row" {
                    ir::Statement::MoveRow(move_item)
                } else {
                    ir::Statement::MoveColumn(move_item)
                });
            }
            if matches!(method, "cancel-flick" | "return-to-bounds")
                && self.owners.get(owner).is_some_and(|target| {
                    !target.instance
                        && target.kind == "Flickable"
                        && !target.callbacks.contains_key(method)
                })
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        format!("{method}() is not allowed in a pure callback"),
                    );
                }
                if !call.arguments.is_empty() {
                    return fail(call.span, format!("{method}() takes no arguments"));
                }
                self.require_static_viewport(owner, call.span)?;
                return Ok(if method == "cancel-flick" {
                    ir::Statement::CancelFlick {
                        flick_viewport: owner.into(),
                        span: call.span,
                    }
                } else {
                    ir::Statement::ReturnToBounds {
                        bounds_viewport: owner.into(),
                        span: call.span,
                    }
                });
            }
            if matches!(
                method,
                "scroll-to"
                    | "position-view-at-index"
                    | "position-view-at-source-index"
                    | "position-view-at-row"
                    | "position-view-at-column"
                    | "position-view-at-cell"
                    | "position-view-at-source-cell"
            ) && let Some(target) = self.owners.get(owner)
                && !target.instance
                && viewport_kind(&target.kind)
                && !target.callbacks.contains_key(method)
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "scroll commands are not allowed in a pure callback",
                    );
                }
                let cell = matches!(
                    method,
                    "position-view-at-cell" | "position-view-at-source-cell"
                );
                let arity = if cell { 4 } else { 2 };
                if call.arguments.len() != arity {
                    return fail(call.span, format!("{method}() takes {arity} arguments"));
                }
                if matches!(
                    method,
                    "position-view-at-index" | "position-view-at-source-index"
                ) && content_viewport(&target.kind)
                {
                    return fail(
                        call.span,
                        format!("{method}() requires a list, grid, tree, or table"),
                    );
                }
                if matches!(
                    method,
                    "position-view-at-row"
                        | "position-view-at-column"
                        | "position-view-at-cell"
                        | "position-view-at-source-cell"
                ) && !matches!(target.kind.as_str(), "TableView" | "StandardTableView")
                {
                    return fail(call.span, format!("{method}() requires a table"));
                }
                self.require_static_viewport(owner, call.span)?;
                let types: &[Type] = if method == "scroll-to" {
                    &[Type::Length, Type::Length]
                } else if cell {
                    &[
                        Type::Int,
                        Type::Int,
                        Type::ScrollAlignment,
                        Type::ScrollAlignment,
                    ]
                } else {
                    &[Type::Int, Type::ScrollAlignment]
                };
                let args = call
                    .arguments
                    .iter()
                    .zip(types)
                    .map(|(arg, ty)| expression_as(arg, self, current, ty.clone()))
                    .collect::<Result<Vec<_>>>()?;
                return Ok(ir::Statement::ScrollCommand(ir::ScrollCommand {
                    viewport: owner.into(),
                    method: method.into(),
                    args,
                    span: call.span,
                }));
            }
            if let Some(target) = self.owners.get(owner)
                && !target.instance
                && !target.callbacks.contains_key(method)
                && ((text_input(&target.kind) || target.kind == "ComboBox")
                    && matches!(
                        method,
                        "select-all"
                            | "clear-selection"
                            | "set-selection-offsets"
                            | "undo"
                            | "redo"
                            | "cut"
                            | "copy"
                            | "paste"
                    )
                    || table_view(&target.kind) && matches!(method, "edit-cell" | "cancel-edit"))
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "text commands are not allowed in a pure callback",
                    );
                }
                let arity = if matches!(method, "set-selection-offsets" | "edit-cell") {
                    2
                } else {
                    0
                };
                if call.arguments.len() != arity {
                    return fail(call.span, format!("{method}() expects {arity} arguments"));
                }
                let mut current_target = Some(owner);
                while let Some(key) = current_target {
                    let element = &self.owners[key];
                    if element.repeated {
                        return fail(
                            call.span,
                            "text commands require a statically addressed editor",
                        );
                    }
                    current_target = element.parent.as_deref();
                }
                let args = call
                    .arguments
                    .iter()
                    .map(|arg| expression_as(arg, self, current, Type::Int))
                    .collect::<Result<Vec<_>>>()?;
                return Ok(ir::Statement::TextCommand(ir::TextCommand {
                    owner: owner.into(),
                    method: method.into(),
                    args,
                    span: call.span,
                }));
            }
            if method == "cancel"
                && self.owners.get(owner).is_some_and(|target| {
                    !target.instance
                        && target.kind == "SwipeGestureHandler"
                        && !target.callbacks.contains_key(method)
                })
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "swipe cancellation is not allowed in a pure callback",
                    );
                }
                if !call.arguments.is_empty() {
                    return fail(call.span, "cancel() takes no arguments");
                }
                let mut ancestor = Some(owner);
                while let Some(key) = ancestor {
                    let element = &self.owners[key];
                    if element.repeated {
                        return fail(
                            call.span,
                            "cancel() requires a statically addressed swipe handler",
                        );
                    }
                    ancestor = element.parent.as_deref();
                }
                return Ok(ir::Statement::SwipeCancel {
                    swipe_owner: owner.into(),
                    span: call.span,
                });
            }
            if matches!(method, "focus" | "clear-focus")
                && let Some(target) = self.owners.get(owner)
                && !target.instance
                && focus_target_kind(&target.kind)
                && !target.callbacks.contains_key(method)
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "focus commands are not allowed in a pure callback",
                    );
                }
                if !call.arguments.is_empty() {
                    return fail(call.span, format!("{method}() takes no arguments"));
                }
                let mut element = target;
                loop {
                    if element.repeated {
                        return fail(
                            call.span,
                            "focus commands require a statically addressed element",
                        );
                    }
                    let Some(parent) = &element.parent else {
                        break;
                    };
                    element = &self.owners[parent];
                }
                return Ok(ir::Statement::FocusCommand(ir::FocusCommand {
                    focus_owner: owner.into(),
                    clear: method == "clear-focus",
                    span: call.span,
                }));
            }
            if matches!(method, "show" | "hide" | "close")
                && self
                    .owners
                    .get(owner)
                    .is_some_and(|target| target.kind == "SecondaryWindow")
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "secondary window commands are not allowed in a pure callback",
                    );
                }
                if !call.arguments.is_empty() {
                    return fail(
                        call.span,
                        format!("SecondaryWindow.{method}() takes no arguments"),
                    );
                }
                return Ok(ir::Statement::SecondaryWindowCommand(
                    ir::SecondaryWindowCommand {
                        secondary_window: owner.into(),
                        command: method.into(),
                        span: call.span,
                    },
                ));
            }
            if owner == "root"
                && method == "close"
                && is_window(&self.owners["root"].kind)
                && !self.owners["root"].callbacks.contains_key("close")
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "Window.close() is not allowed in a pure callback",
                    );
                }
                if !call.arguments.is_empty() {
                    return fail(call.span, "Window.close() takes no arguments");
                }
                return Ok(ir::Statement::WindowClose {
                    window_close: call.span,
                });
            }
            if owner == "root"
                && is_window(&self.owners["root"].kind)
                && matches!(
                    method,
                    "minimize"
                        | "maximize"
                        | "restore"
                        | "toggle-maximized"
                        | "set-full-screen"
                        | "toggle-full-screen"
                )
                && !self.owners["root"].callbacks.contains_key(method)
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "window commands are not allowed in a pure callback",
                    );
                }
                let argument = match (method, call.arguments.as_slice()) {
                    ("set-full-screen", [enabled]) => {
                        Some(expression_as(enabled, self, current, Type::Bool)?)
                    }
                    ("set-full-screen", _) => {
                        return fail(call.span, "set-full-screen() takes one bool argument");
                    }
                    (_, []) => None,
                    _ => return fail(call.span, format!("{method}() takes no arguments")),
                };
                return Ok(ir::Statement::WindowCommand(ir::WindowCommand {
                    window_command: method.into(),
                    argument,
                    span: call.span,
                }));
            }
            if method == "open"
                && let Some(target) = self
                    .owners
                    .get(owner)
                    .filter(|target| native_dialog(&target.kind))
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        format!("{}.open() is not allowed in a pure callback", target.kind),
                    );
                }
                if !call.arguments.is_empty() {
                    return fail(
                        call.span,
                        format!("{}.open() takes no arguments", target.kind),
                    );
                }
                return Ok(ir::Statement::NativeDialogOpen(ir::NativeDialogOpen {
                    dialog: owner.into(),
                    span: call.span,
                }));
            }
            if self
                .owners
                .get(owner)
                .is_some_and(|target| target.kind == "PopupWindow")
                && matches!(method, "show" | "close")
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "popup commands are not allowed in a pure callback",
                    );
                }
                if !call.arguments.is_empty() {
                    return fail(
                        call.span,
                        format!("popup command `{method}` takes no arguments"),
                    );
                }
                return Ok(ir::Statement::Popup(ir::PopupEffect {
                    owner: owner.into(),
                    open: method == "show",
                    span: call.span,
                }));
            }
            if self
                .owners
                .get(owner)
                .is_some_and(|target| target.kind == "MediaPlayer")
                && matches!(method, "play" | "pause" | "stop" | "seek" | "go-live")
            {
                if !self.effects_allowed {
                    return fail(call.span, "MediaPlayer commands require an impure callback");
                }
                let position = match (method, call.arguments.as_slice()) {
                    ("seek", [position]) => {
                        Some(expression_as(position, self, current, Type::Float)?)
                    }
                    ("seek", _) => return fail(call.span, "seek() takes one position in seconds"),
                    (_, []) => None,
                    _ => return fail(call.span, format!("{method}() takes no arguments")),
                };
                return Ok(ir::Statement::Media(ir::MediaEffect {
                    owner: owner.into(),
                    method: method.into(),
                    position,
                    span: call.span,
                }));
            }
            if self
                .owners
                .get(owner)
                .is_some_and(|target| target.kind == "Timer")
                && matches!(method, "start" | "stop" | "restart")
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "timer commands are not allowed in a pure callback",
                    );
                }
                if !call.arguments.is_empty() {
                    return fail(
                        call.span,
                        format!("timer command `{method}` takes no arguments"),
                    );
                }
                return Ok(ir::Statement::Timer(ir::TimerEffect {
                    owner: owner.into(),
                    command: method.into(),
                    span: call.span,
                }));
            }
            if self
                .owners
                .get(owner)
                .is_some_and(|target| target.kind == "StackView")
                && matches!(
                    method,
                    "push" | "pop" | "replace" | "pop-to" | "reset" | "push-pages" | "reset-to"
                )
            {
                if !self.effects_allowed {
                    return fail(
                        call.span,
                        "stack navigation is not allowed in a pure callback",
                    );
                }
                let expected = usize::from(method != "pop");
                if call.arguments.len() != expected {
                    return fail(
                        call.span,
                        format!("stack navigation `{method}` expects {expected} arguments"),
                    );
                }
                let path = matches!(method, "push-pages" | "reset-to");
                let page = call
                    .arguments
                    .first()
                    .map(|page| {
                        expression_as(
                            page,
                            self,
                            current,
                            if path { Type::StringList } else { Type::String },
                        )
                    })
                    .transpose()?;
                if let Some(page) = &page {
                    if path {
                        self.check_stack_path(owner, page)?;
                    } else {
                        self.check_stack_route(owner, page)?;
                    }
                }
                return Ok(ir::Statement::Navigate(ir::Navigation {
                    owner: owner.into(),
                    method: method.into(),
                    page,
                    span: call.span,
                }));
            }
        }
        Ok(ir::Statement::Invoke(self.call(call, current)?.0))
    }
}

fn as_property(member: &Member) -> Option<&ast::Property> {
    match member {
        Member::Property(p) => Some(p),
        _ => None,
    }
}

pub fn control_alias_type(kind: &str, name: &str) -> Option<Type> {
    let kind = delegate_base(kind).unwrap_or(kind);
    Some(match (kind, name) {
        (kind, "text") if text_input(kind) => Type::String,
        ("Switch" | "CheckBox", "checked") => Type::Bool,
        ("Slider" | "Dial" | "DoubleSpinBox", "value") => Type::Float,
        ("RangeSlider", "lower-value" | "upper-value") => Type::Float,
        ("SpinBox", "value") => Type::Int,
        _ => return None,
    })
}

fn alias_initializer(p: &ast::Property, scope: &Scope<'_>, owner: &str) -> Result<ir::Expr> {
    let Some(ast::Expr {
        kind: ExprKind::Reference(reference),
        span,
    }) = &p.value
    else {
        return fail(p.span, "property alias requires a property reference");
    };
    let (target, name) =
        scope.member_owner(owner, reference, "property", *span, |scope, name| {
            scope.properties.contains_key(name)
        })?;
    if target.starts_with("__global_") {
        return fail(
            *span,
            "use a binding or an assignment to access a global property",
        );
    }
    for start in [owner, target] {
        let mut current = Some(start);
        while let Some(key) = current {
            let element = &scope.owners[key];
            if element.repeated {
                return fail(
                    *span,
                    "property aliases require statically addressed endpoints",
                );
            }
            current = element.parent.as_deref();
        }
    }
    let element = &scope.owners[target];
    let tristate = element.kind == "CheckBox" && element.members.iter().any(|member| {
        matches!(member, Member::Binding(binding) if matches!(binding.name.as_str(), "check-state" | "tristate"))
    });
    let value = if let Some(ty) =
        control_alias_type(&element.kind, name).filter(|_| !element.instance && !tristate)
    {
        ir::Expr {
            op: "control_read".into(),
            owner: Some(target.into()),
            value: name.into(),
            ty,
            args: vec![],
            span: *span,
        }
    } else {
        let (target, ty) = scope.property(owner, reference, true, *span)?;
        ir::Expr::read(target, ty, *span)
    };
    if value.ty != p.ty {
        return fail(
            *span,
            format!(
                "property alias type mismatch: expected {}, found {}",
                p.ty.name(),
                value.ty.name()
            ),
        );
    }
    Ok(value)
}

fn property(p: &ast::Property, scope: &Scope<'_>, owner: &str) -> Result<ir::Property> {
    let caller = scope.for_caller(owner, &p.name);
    let scope = caller.as_ref().unwrap_or(scope);
    let init = match &p.value {
        _ if p.two_way => alias_initializer(p, scope, owner)?,
        Some(e) => expression_as(e, scope, owner, p.ty.clone())?,
        None => default(p.ty.clone(), p.span),
    };
    Ok(ir::Property {
        name: p.name.clone(),
        ty: p.ty.clone(),
        visibility: p.visibility.clone(),
        init,
        alias: p.two_way,
        span: p.span,
    })
}

fn lower_callable(
    info: &ResolvedCallback,
    handler: Option<&ast::Handler>,
    scope: &Scope<'_>,
    owner: &str,
) -> Result<ir::Callback> {
    let definition = &info.signature;
    if let Some(handler) = handler
        && handler.parameters.len() != definition.parameters.len()
    {
        return fail(
            handler.span,
            format!(
                "callback `{}` expects {} parameters",
                definition.name,
                definition.parameters.len()
            ),
        );
    }
    let span = handler.map_or(definition.span, |handler| handler.span);
    let mut names = BTreeSet::new();
    let parameters = definition
        .parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| {
            let name = handler.map_or(parameter.name.as_str(), |handler| {
                handler.parameters[index].as_str()
            });
            let name = if name.is_empty() {
                format!("__xr_arg_{index}")
            } else {
                check_name(name, span)?;
                if !names.insert(rust_name(name)) {
                    return fail(span, format!("duplicate handler parameter `{name}`"));
                }
                name.into()
            };
            Ok(ast::Parameter {
                name,
                ty: parameter.ty.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut callback = ir::Callback {
        name: definition.name.clone(),
        parameters,
        span: definition.span,
        pure: definition.pure,
        body: vec![],
        result: definition.return_type.clone().map(|ty| {
            if info.builtin
                && matches!(
                    definition.name.as_str(),
                    "close-requested" | "leave-requested"
                )
            {
                ir::Expr::literal(Type::Bool, "true", span)
            } else {
                default(ty, span)
            }
        }),
        alias: info.target.clone(),
        has_handler: handler.is_some(),
    };
    if let Some(handler) = handler {
        if !info.builtin {
            scope.reference(
                handler.span,
                &handler.name,
                0,
                definition.span,
                &definition.name,
            );
        }
        let mut handler_scope = scope
            .for_caller(owner, &handler.name)
            .unwrap_or_else(|| scope.clone());
        handler_scope.effects_allowed = !callback.pure;
        handler_scope.record_handler_parameters(handler);
        handler_scope.parameters.extend(
            callback
                .parameters
                .iter()
                .map(|parameter| (parameter.name.clone(), parameter.ty.clone())),
        );
        callback.body = statements(handler, &handler_scope, owner)?;
        callback.result = match (definition.return_type.clone(), &handler.result) {
            (Some(ty), Some(value)) => Some(expression_as(value, &handler_scope, owner, ty)?),
            (Some(ty), None) => return fail(span, format!("callback must return {}", ty.name())),
            (None, Some(value)) => {
                callback
                    .body
                    .push(discarded_call(value, &handler_scope, owner)?);
                None
            }
            (None, None) => None,
        };
    }
    Ok(callback)
}

fn lower_local_callbacks<'a>(
    members: &[Member],
    scope: &Scope<'_>,
    owner: &str,
    lowered: &mut Vec<LoweredMember<'a>>,
) -> Result<()> {
    for info in scope.local_callbacks(owner) {
        let mut handlers = members.iter().filter_map(|member| match member {
            Member::Handler(handler) if handler.name == info.signature.name => Some(handler),
            _ => None,
        });
        let handler = handlers.next();
        if let Some(duplicate) = handlers.next() {
            return fail(
                duplicate.span,
                format!("duplicate handler `{}`", duplicate.name),
            );
        }
        lowered.push(LoweredMember::Callback(lower_callable(
            info, handler, scope, owner,
        )?));
    }
    Ok(())
}

fn event_relay(info: &ResolvedCallback, owner: &str) -> Result<ir::Callback> {
    let mut callback = lower_callable(info, None, &Scope::default(), owner)?;
    let args = callback
        .parameters
        .iter()
        .map(|p| ir::Expr {
            op: "param".into(),
            value: p.name.clone(),
            ty: p.ty.clone(),
            owner: None,
            args: vec![],
            span: callback.span,
        })
        .collect::<Vec<_>>();
    if let Some(result) = &mut callback.result {
        *result = ir::Expr {
            op: "call".into(),
            value: callback.name.clone(),
            ty: result.ty.clone(),
            owner: Some(owner.into()),
            args,
            span: callback.span,
        };
    } else {
        callback.body.push(ir::Statement::Invoke(ir::CallbackCall {
            owner: owner.into(),
            name: callback.name.clone(),
            args,
            span: callback.span,
        }));
    }
    Ok(callback)
}

fn custom_clipboard_paste(
    members: &[Member],
    scope: &Scope<'_>,
    owner: &str,
) -> Result<Option<ir::ClipboardPaste>> {
    let target = members.iter().find_map(|member| match member {
        Member::Binding(binding) if binding.name == "clipboard-target" => Some(binding),
        _ => None,
    });
    let handler = members.iter().find_map(|member| match member {
        Member::Handler(handler) if handler.name == "clipboard-paste" => Some(handler),
        _ => None,
    });
    let (target, handler) = match (target, handler) {
        (None, None) => return Ok(None),
        (Some(target), Some(handler)) => (target, handler),
        (Some(target), None) => {
            return fail(
                target.span,
                "clipboard-target requires a clipboard-paste handler",
            );
        }
        (None, Some(handler)) => {
            return fail(handler.span, "clipboard-paste requires clipboard-target");
        }
    };
    let ExprKind::Literal {
        value,
        ty: Type::String,
    } = &target.value.kind
    else {
        return fail(
            target.span,
            "clipboard-target requires a constant property name",
        );
    };
    if target.two_way {
        return fail(
            target.span,
            "clipboard-target requires a constant property name",
        );
    }
    let name: String = serde_json::from_str(value).map_err(|error| Diagnostic {
        span: target.span,
        message: error.to_string(),
    })?;
    let row = scope
        .repeat_source
        .as_ref()
        .expect("table row has a repeat");
    let declaration = scope.owners[owner].properties.get(name.as_str());
    let linked = members.iter().any(|member| {
        matches!(member, Member::Binding(binding)
        if binding.name == name && binding.two_way
        && matches!(&binding.value.kind, ExprKind::Reference(reference)
            if reference.split('.').next() == Some(row.item.as_str())))
    });
    let Some(declaration) =
        declaration.filter(|property| property.visibility == "in-out" && linked)
    else {
        return fail(
            target.span,
            "clipboard-target must name an in-out property linked to the current model row",
        );
    };
    let [text] = handler.parameters.as_slice() else {
        return fail(handler.span, "clipboard-paste expects one string parameter");
    };
    check_name(text, handler.span)?;
    if scope
        .parameters
        .keys()
        .any(|name| rust_name(name) == rust_name(text))
    {
        return fail(
            handler.span,
            "clipboard-paste parameter must differ from the row item and index",
        );
    }
    let Some(result) = handler
        .result
        .as_ref()
        .filter(|_| handler.statements.is_empty())
    else {
        return fail(
            handler.span,
            "clipboard-paste requires one pure result expression",
        );
    };
    let mut parser = scope.clone();
    parser.effects_allowed = false;
    parser.parameters.insert(text.clone(), Type::String);
    parser.owners.retain(|key, _| {
        let mut current = Some(key.as_str());
        while let Some(key) = current {
            let element = &scope.owners[key];
            if element.repeated {
                return false;
            }
            current = element.parent.as_deref();
        }
        true
    });
    let result = expression(result, &parser, "root")?;
    let schema = match &result.ty {
        Type::Record(name) => scope.records.get(name.as_ref()).copied(),
        _ => None,
    };
    if !schema.is_some_and(|record| {
        record.fields.len() == 2
            && record
                .fields
                .iter()
                .any(|field| field.name == "accepted" && field.ty == Type::Bool)
            && record
                .fields
                .iter()
                .any(|field| field.name == "value" && field.ty == declaration.ty)
    }) {
        return fail(
            result.span,
            format!(
                "clipboard-paste must return a record with accepted: bool and value: {}",
                declaration.ty.name()
            ),
        );
    }
    Ok(Some(ir::ClipboardPaste {
        target: name,
        text_parameter: text.clone(),
        result,
        span: handler.span,
    }))
}

fn component<'a>(c: &'a ast::Component, scope: Scope<'a>) -> Result<Vec<LoweredMember<'a>>> {
    check_name(&c.name, c.span)?;
    if c.global {
        if let Some(member) = c.members.iter().find(|member| {
            !matches!(
                member,
                Member::Property(_) | Member::Callback(_) | Member::Handler(_)
            )
        }) {
            return fail(
                member.span(),
                "global only accepts properties, callbacks and handlers",
            );
        }
    }
    if is_builtin(&c.name) {
        return fail(
            c.span,
            format!(
                "component name `{}` conflicts with a built-in element",
                c.name
            ),
        );
    }
    if !is_builtin(&c.base) {
        return fail(
            c.span,
            format!(
                "unknown base element `{}`; component inheritance is not supported yet",
                c.base
            ),
        );
    }
    if matches!(c.base.as_str(), "Tooltip" | "PopupWindow") {
        return fail(c.span, format!("{} requires a visual owner", c.base));
    }
    if swipe_slot(&c.base) {
        return fail(c.span, "swipe slots require a direct SwipeDelegate parent");
    }
    if let Some(parent) = control_part_parent(&c.base) {
        return fail(
            c.span,
            format!("{} requires a direct {parent} parent", c.base),
        );
    }
    if c.base == "SplitHandle" {
        return fail(c.span, "SplitHandle requires a direct SplitView parent");
    }
    if matches!(c.base.as_str(), "PageHeader" | "PageFooter") {
        return fail(
            c.span,
            format!(
                "{} requires a direct Page parent or ApplicationWindow parent",
                c.base
            ),
        );
    }
    if c.base == "Action" {
        return fail(
            c.span,
            "Action requires a named declaration inside a component",
        );
    }
    if matches!(
        c.base.as_str(),
        "Timer" | "ActionGroup" | "ButtonGroup" | "SecondaryWindow"
    ) || native_dialog(&c.base)
    {
        return fail(
            c.span,
            format!("{} requires a named declaration inside a component", c.base),
        );
    }
    if matches!(c.base.as_str(), "Menu" | "MenuItem" | "MenuSeparator") {
        return fail(
            c.span,
            "menu definitions require a MenuBar, Menu, or ContextMenuArea parent",
        );
    }
    if matches!(
        c.base.as_str(),
        "Tab"
            | "TabButton"
            | "SwipePage"
            | "StackPage"
            | "RadioButton"
            | "RadioDelegate"
            | "DayCell"
            | "TableColumn"
            | "TableRow"
            | "TableCell"
            | "TreeRow"
            | "TreeCell"
    ) {
        return fail(c.span, "component root cannot be a tab or radio option");
    }
    let mut lowered = Vec::new();
    for p in c.members.iter().filter_map(as_property) {
        lowered.push(LoweredMember::Property(property(p, &scope, "root")?));
    }
    for member in &c.members {
        if let Member::Handler(h) = member
            && scope.callback_info("root", &h.name).is_none()
            && !control_event(&c.base, &h.name)
        {
            return fail(h.span, format!("unknown callback `{}`", h.name));
        }
    }
    lower_local_callbacks(&c.members, &scope, "root", &mut lowered)?;
    element(
        (&c.base, Some("root"), c.span),
        &c.members,
        &scope,
        "root",
        None,
        None,
        &mut lowered,
    )?;
    Ok(lowered)
}

fn element<'a>(
    (kind, id, span): (&'a str, Option<&'a str>, Span),
    members: &'a [Member],
    scope: &Scope<'_>,
    owner: &str,
    condition: Option<&ast::Expr>,
    repeat: Option<&ast::Repeat>,
    lowered: &mut Vec<LoweredMember<'a>>,
) -> Result<()> {
    let source_kind = kind;
    let delegate = delegate_base(kind).map(|_| kind);
    let kind = delegate_base(kind).unwrap_or(kind);
    let visual_template_scope;
    let scope = if matches!(kind, "DayCell" | "SplitHandle") {
        validate_visual_template(kind, members)?;
        let mut template = scope.clone();
        let inputs = if kind == "DayCell" {
            day_cell_inputs().to_vec()
        } else {
            split_handle_inputs().to_vec()
        };
        template
            .parameters
            .extend(inputs.into_iter().map(|(name, ty)| (name.into(), ty)));
        visual_template_scope = template;
        &visual_template_scope
    } else {
        scope
    };
    let virtual_delegate = delegate.is_some()
        && repeat.is_some()
        && scope.owners[owner].parent.as_deref().is_some_and(|parent| {
            matches!(scope.owners[parent].kind.as_str(), "ListView" | "GridView")
        });
    let root = owner == "root";
    let instance = scope.owners[owner].instance;
    let secondary_content = scope.owners[owner]
        .parent
        .as_deref()
        .is_some_and(|parent| scope.owners[parent].kind == "SecondaryWindow");
    if kind == "TreeCell" && (scope.row_template != Some(RowTemplate::Tree) || repeat.is_some()) {
        return fail(
            span,
            "TreeCell requires a stable child path within a TreeRow",
        );
    }
    if matches!(kind, "AnimatedImage" | "MediaPlayer") {
        let mut current = Some(owner);
        while let Some(key) = current {
            let element = &scope.owners[key];
            if element.repeated {
                return fail(
                    span,
                    format!("{kind} in a repetition requires a component delegate"),
                );
            }
            current = element.parent.as_deref();
        }
    }
    lowered.push(LoweredMember::BeginElement {
        kind,
        delegate,
        id,
        key: owner.into(),
        instance,
        span,
    });
    let action_group = scope.action_groups.members.get(owner);
    if let Some(membership) = action_group {
        lowered.push(LoweredMember::ActionGroupRef {
            membership: membership.clone(),
            span,
        });
    }
    if let Some(initial) = scope.action_groups.initial.get(owner) {
        lowered.push(LoweredMember::Binding(ir::Binding {
            name: "current-value".into(),
            value: ir::Expr::literal(Type::String, serde_json::to_string(initial).unwrap(), span),
            span,
        }));
    }
    if let Some(condition) = condition {
        let parent = scope.owners[owner]
            .placement
            .and_then(|placement| placement.caller.as_ref().map(|caller| caller.1.as_str()))
            .or_else(|| scope.lexical_parent(owner))
            .unwrap_or("root");
        lowered.push(LoweredMember::Condition(expression_as(
            condition,
            scope,
            parent,
            Type::Bool,
        )?));
    }
    let repeated_scope;
    let scope = if let Some(repeat) = repeat {
        if virtual_delegate {
            if id.is_some()
                || members.iter().any(|member| {
                    !matches!(
                        member,
                        Member::Binding(_)
                            | Member::Animation(_)
                            | Member::States(_)
                            | Member::Handler(_)
                    )
                })
            {
                return fail(
                    span,
                    "virtual item delegates require an anonymous leaf template",
                );
            }
            if matches!(kind, "CheckBox" | "Switch")
                && !members.iter().any(|member| {
                    matches!(member, Member::Binding(binding)
                    if binding.two_way && row_editor_type(kind, &binding.name).is_some())
                })
            {
                return fail(
                    span,
                    "a virtual toggle delegate requires a two-way binding to a writable row field",
                );
            }
        } else if !instance {
            validate_template(
                kind,
                id,
                members,
                span,
                kind == "TreeRow",
                matches!(kind, "TableRow" | "TreeRow"),
            )?;
        }
        check_name(&repeat.item, span)?;
        if let Some(index) = &repeat.index {
            check_name(index, span)?;
            if rust_name(index) == rust_name(&repeat.item) {
                return fail(span, "repeat item and index must have different names");
            }
        }
        let parent = scope.owners[owner]
            .placement
            .and_then(|placement| placement.caller.as_ref().map(|caller| caller.1.as_str()))
            .or_else(|| scope.lexical_parent(owner))
            .unwrap_or("root");
        let model = expression(&repeat.model, scope, parent)?;
        if kind == "TreeRow" {
            if !matches!(model.ty, Type::RecordList(_)) {
                return fail(model.span, "TreeRow requires a record list model");
            }
            for role in ["key", "parent-key"] {
                if !members.iter().any(|member| matches!(member, Member::Binding(binding)
                    if binding.name == role && !binding.two_way && matches!(&binding.value.kind, ExprKind::Reference(reference)
                        if reference.split_once('.').is_some_and(|(item, _)| item == repeat.item))))
                {
                    return fail(span, format!("TreeRow {role} requires an integer field of the repeated record"));
                }
            }
        }
        let item_type = if model.ty == Type::Int {
            Type::Int
        } else if let Some(item) = model.ty.list_item() {
            item
        } else {
            return fail(model.span, "repeat expects an integer or a list model");
        };
        let mut template = scope.clone();
        template.row_template = match kind {
            "TableRow" => Some(RowTemplate::Table),
            "TreeRow" => Some(RowTemplate::Tree),
            _ if virtual_delegate => Some(RowTemplate::List),
            _ => None,
        };
        template.repeat_source = Some(RepeatSource {
            item: repeat.item.clone(),
            owner: parent.into(),
            property: match &repeat.model.kind {
                ExprKind::Reference(reference) if model.op == "read" => {
                    Some((reference.clone(), repeat.model.span))
                }
                _ => None,
            },
        });
        template.writable_row = match &repeat.model.kind {
            ExprKind::Reference(reference)
                if template.row_template.is_some()
                    && model.op == "read"
                    && matches!(model.ty, Type::StringList | Type::RecordList(_))
                    && scope.property(parent, reference, true, model.span).is_ok() =>
            {
                Some(repeat.item.clone())
            }
            _ => None,
        };
        template.parameters.insert(repeat.item.clone(), item_type);
        if template.references.is_some() {
            template
                .parameter_sites
                .insert(repeat.item.clone(), repeat.item_span);
            if let Some(index) = &repeat.index {
                template
                    .parameter_sites
                    .insert(index.clone(), repeat.index_span);
            }
        }
        if let Some(index) = &repeat.index {
            template.parameters.insert(index.clone(), Type::Int);
        }
        lowered.push(LoweredMember::Repeat {
            model,
            item: repeat.item.clone(),
            index: repeat.index.clone(),
        });
        repeated_scope = template;
        &repeated_scope
    } else {
        scope
    };
    let state_scope;
    let scope = if let Some((selection, targets)) = lower_states(members, scope, owner, span)? {
        lowered.push(LoweredMember::StateSelection(selection));
        let mut nested = scope.clone();
        for (owner, state) in targets {
            let target = nested.state_targets.entry(owner).or_default();
            target.bindings.extend(state.bindings);
            target.timings.extend(state.timings);
        }
        state_scope = nested;
        &state_scope
    } else {
        scope
    };
    if let Some(state) = scope.state_targets.get(owner) {
        lowered.extend(
            state
                .bindings
                .iter()
                .cloned()
                .map(LoweredMember::StateBinding),
        );
        lowered.extend(
            state
                .timings
                .iter()
                .cloned()
                .map(LoweredMember::StateTiming),
        );
    }
    let mut bindings = BTreeSet::new();
    let mut handlers = BTreeSet::new();
    let mut animations = BTreeSet::new();
    let mut range_target = None;
    let mut viewport_target = None;
    let mut calendar_target = None;
    let mut combo_text_target = None;
    let text_role = if kind == "StandardListView" {
        standard_list_role(members)?
    } else {
        None
    };
    if !instance && row_height_view(kind) {
        check_view_record_field(
            kind,
            members,
            scope,
            owner,
            "row-height-field",
            Type::Length,
        )?;
    }
    if !instance && row_visibility_view(kind) {
        check_view_record_field(kind, members, scope, owner, "row-visible-field", Type::Bool)?;
    }
    if !instance && matches!(kind, "TreeView" | "StandardTableView") {
        let has_binding = |name| {
            members
                .iter()
                .any(|member| matches!(member, Member::Binding(binding) if binding.name == name))
        };
        if has_binding("filter-text") && !has_binding("filter-field") {
            return fail(span, format!("{kind} filter-text requires filter-field"));
        }
        check_view_record_field(kind, members, scope, owner, "filter-field", Type::String)?;
    }
    let combo = if kind == "ComboBox" {
        record_combo(members, scope, owner)?
    } else {
        None
    };
    let has_action = matches!(
        kind,
        "Button" | "ToolButton" | "RoundButton" | "MenuItem" | "Shortcut"
    ) && members
        .iter()
        .any(|member| matches!(member, Member::Binding(binding) if binding.name == "action"));
    let clipboard = if instance && scope.row_template == Some(RowTemplate::Table) {
        custom_clipboard_paste(members, scope, owner)?
    } else {
        None
    };
    let clipboard_parser = clipboard.is_some();
    let scroll_control = scroll_control(kind);
    let attached_scroll = scroll_control && has_scroll_target(members);
    if let Some(clipboard) = clipboard {
        lowered.push(LoweredMember::ClipboardPaste(clipboard));
    }
    if !instance {
        if !root {
            lower_local_callbacks(members, scope, owner, lowered)?;
        }
        for event in scope.local_callbacks(owner).filter(|info| info.builtin) {
            lowered.push(LoweredMember::Handler(event_relay(event, owner)?));
        }
    }
    for (index, m) in members.iter().enumerate() {
        if scroll_control && matches!(m, Member::Element(_)) {
            return fail(m.span(), format!("{kind} cannot contain child elements"));
        }
        if attached_scroll && matches!(m, Member::Handler(h) if control_event(kind, &h.name)) {
            return fail(
                m.span(),
                "handle scrolling on the target viewport with scrolled()",
            );
        }
        if kind == "ButtonGroup" && !matches!(m, Member::Binding(_) | Member::Handler(_)) {
            return fail(
                m.span(),
                "ButtonGroup only accepts property bindings and selected(value)",
            );
        }
        if kind == "ButtonGroup"
            && non_exclusive_group(members)
            && matches!(m, Member::Handler(handler) if handler.name == "selected")
        {
            return fail(
                m.span(),
                "selected(value) requires an exclusive ButtonGroup",
            );
        }
        if kind == "Action" && !matches!(m, Member::Binding(_) | Member::Handler(_)) {
            return fail(
                m.span(),
                "Action only accepts property bindings and activated()",
            );
        }
        if kind == "ActionGroup"
            && !matches!(m, Member::Binding(_) | Member::Handler(_))
            && !matches!(m, Member::Element(child) if child.ty == "Action")
        {
            return fail(
                m.span(),
                "ActionGroup only accepts bindings, Actions and activated(value)",
            );
        }
        if kind == "Timer" && !matches!(m, Member::Binding(_) | Member::Handler(_)) {
            return fail(
                m.span(),
                "Timer only accepts property bindings and triggered()",
            );
        }
        if native_dialog(kind) && !matches!(m, Member::Binding(_) | Member::Handler(_)) {
            return fail(
                m.span(),
                format!("{kind} only accepts bindings and result handlers"),
            );
        }
        if kind == "SecondaryWindow" && !matches!(m, Member::Binding(_) | Member::Element(_)) {
            return fail(
                m.span(),
                "SecondaryWindow only accepts visibility and a Window instance",
            );
        }
        if kind == "Shortcut" && !matches!(m, Member::Binding(_) | Member::Handler(_)) {
            return fail(
                m.span(),
                "Shortcut only accepts bindings and activation handlers",
            );
        }
        if instance
            && !matches!(
                m,
                Member::Binding(_) | Member::Handler(_) | Member::States(_)
            )
        {
            return fail(
                m.span(),
                "a component instance body only supports inputs, layout bindings, states, and callback handlers",
            );
        }
        match m {
            Member::Binding(b) => {
                let caller = scope.for_caller(owner, &b.name);
                let scope = caller.as_ref().unwrap_or(scope);
                if virtual_delegate && kind == "RadioButton" && b.name == "checked" {
                    return fail(
                        b.span,
                        "virtual RadioDelegate checked state follows the view's current-index",
                    );
                }
                if kind == "ButtonGroup" {
                    if b.name == "check-state" {
                        return fail(
                            b.span,
                            "ButtonGroup check-state is read-only; use set-check-state()",
                        );
                    }
                    if non_exclusive_group(members)
                        && matches!(b.name.as_str(), "current-index" | "current-value")
                    {
                        return fail(
                            b.span,
                            "current-index and current-value require an exclusive ButtonGroup",
                        );
                    }
                }
                if kind == "StackPage" && b.name == "visible" {
                    return fail(b.span, "StackView manages StackPage visibility");
                }
                let viewport_offset = viewport_offset_property(kind, &b.name);
                if geometry_property(&b.name).is_some()
                    || scale_rotate_property(kind, &b.name).is_some()
                    || swipe_property(kind, &b.name).is_some()
                    || hover_property(kind, &b.name).is_some()
                    || touch_area_property(kind, &b.name).is_some()
                    || kind == "SplitView" && b.name == "resizing"
                    || flick_motion_property(kind, &b.name).is_some()
                    || focus_property(kind, &b.name)
                    || is_window(kind) && window_status_property(&b.name)
                    || viewport_property(kind, &b.name) && !viewport_offset
                {
                    return fail(b.span, format!("property `{kind}.{}` is read-only", b.name));
                }
                if viewport_offset {
                    scope.require_static_viewport(owner, b.span)?;
                }
                if kind == "AnimatedImage" && playback_property(&b.name) {
                    return fail(
                        b.span,
                        format!("property `AnimatedImage.{}` is read-only", b.name),
                    );
                }
                if kind == "MediaPlayer" && media_read_type(&b.name).is_some() {
                    return fail(
                        b.span,
                        format!("property `MediaPlayer.{}` is read-only", b.name),
                    );
                }
                if table_sort_property(kind, &b.name).is_some()
                    || table_selection_property(kind, &b.name).is_some()
                    || table_history_property(kind, &b.name).is_some()
                {
                    return fail(b.span, format!("property `{kind}.{}` is read-only", b.name));
                }
                if kind == "StackView" && stack_read_type(&b.name).is_some()
                    || kind == "StackPage" && stack_page_read_type(&b.name).is_some()
                {
                    return fail(b.span, format!("property `{kind}.{}` is read-only", b.name));
                }
                if !bindings.insert(b.name.as_str()) {
                    return fail(b.span, format!("duplicate binding `{}`", b.name));
                }
                if let Some(membership) = action_group
                    && (matches!(b.name.as_str(), "group" | "value")
                        || b.name == "checked"
                            && !non_exclusive_group(scope.owners[&membership.group].members))
                {
                    continue;
                }
                if kind == "ActionGroup"
                    && b.name == "current-value"
                    && non_exclusive_group(members)
                {
                    return fail(
                        b.span,
                        "ActionGroup current-value requires an exclusive group",
                    );
                }
                if scroll_control && b.name == "target" {
                    if !b.two_way
                        && let ExprKind::Reference(target) = &b.value.kind
                        && scope.owners.get(target).is_some_and(|element| {
                            !element.instance && viewport_kind(&element.kind)
                        })
                    {
                        scope.require_static_viewport(owner, b.span)?;
                        scope.require_static_viewport(target, b.span)?;
                        lowered.push(LoweredMember::ScrollTarget {
                            target: target.clone(),
                            span: b.span,
                        });
                        continue;
                    }
                    return fail(
                        b.span,
                        "target requires the id of a viewport in this component",
                    );
                }
                if kind == "SplitView" && b.name == "fill-pane" {
                    if !b.two_way
                        && let ExprKind::Reference(target) = &b.value.kind
                        && scope.owners.get(target).is_some_and(|pane| {
                            pane.parent.as_deref() == Some(owner)
                                && !pane.repeated
                                && !nonvisual_element(&pane.kind)
                                && !matches!(
                                    pane.kind.as_str(),
                                    "Menu"
                                        | "Tooltip"
                                        | "Dialog"
                                        | "Drawer"
                                        | "PopupWindow"
                                        | "Shortcut"
                                )
                        })
                    {
                        scope.require_static_viewport(owner, b.span)?;
                        lowered.push(LoweredMember::SplitFill {
                            target: target.clone(),
                            span: b.span,
                        });
                        continue;
                    }
                    return fail(
                        b.span,
                        "fill-pane requires a named direct pane of this SplitView",
                    );
                }
                if attached_scroll && matches!(b.name.as_str(), "position" | "size") {
                    return fail(
                        b.span,
                        "attached scroll controls derive their range from the viewport",
                    );
                }
                if clipboard_parser && b.name == "clipboard-target" {
                    continue;
                }
                if b.name == "clipboard-text"
                    && (!instance
                        || scope.owners[owner]
                            .parent
                            .as_deref()
                            .is_some_and(|parent| scope.owners[parent].kind == "TableRow"))
                {
                    if b.two_way
                        || !scope.owners[owner]
                            .parent
                            .as_deref()
                            .is_some_and(|parent| scope.owners[parent].kind == "TableRow")
                    {
                        return fail(
                            b.span,
                            "clipboard-text requires a string binding on a direct table cell",
                        );
                    }
                    let value = expression_as(&b.value, scope, owner, Type::String)?;
                    lowered.push(LoweredMember::Binding(ir::Binding {
                        name: b.name.clone(),
                        value,
                        span: b.span,
                    }));
                    continue;
                }
                if b.name == "initial-focus" && kind == "FocusScope" {
                    let ExprKind::Reference(target) = &b.value.kind else {
                        return fail(b.span, "initial-focus requires a descendant element id");
                    };
                    let Some(element) = scope
                        .owners
                        .get(target)
                        .filter(|element| !element.instance && focus_target_kind(&element.kind))
                    else {
                        return fail(
                            b.span,
                            "initial-focus requires a focusable descendant element",
                        );
                    };
                    let mut current = element;
                    if target == owner {
                        return fail(b.span, "initial-focus cannot refer to the scope itself");
                    }
                    loop {
                        if current.repeated || current.instance {
                            return fail(
                                b.span,
                                "initial-focus cannot cross a component or repetition boundary",
                            );
                        }
                        let Some(parent) = &current.parent else {
                            return fail(b.span, "initial-focus requires a descendant element");
                        };
                        if parent == owner {
                            break;
                        }
                        current = &scope.owners[parent];
                    }
                    if b.two_way {
                        return fail(b.span, "initial-focus cannot be two-way");
                    }
                    lowered.push(LoweredMember::InitialFocus {
                        target: target.clone(),
                        span: b.span,
                    });
                    continue;
                }
                if b.name == "action"
                    && matches!(
                        kind,
                        "Button" | "ToolButton" | "RoundButton" | "MenuItem" | "Shortcut"
                    )
                {
                    if !b.two_way
                        && let ExprKind::Reference(reference) = &b.value.kind
                        && scope
                            .owners
                            .get(reference)
                            .is_some_and(|target| target.kind == "Action")
                    {
                        lowered.push(LoweredMember::ActionRef {
                            owner: reference.clone(),
                            span: b.span,
                        });
                        continue;
                    }
                    return fail(
                        b.span,
                        "action requires the id of an Action in this component",
                    );
                }
                if matches!(kind, "RadioButton" | "CheckBox" | "Switch") && b.name == "group" {
                    let mut current = &scope.owners[owner];
                    if current
                        .parent
                        .as_deref()
                        .is_some_and(|parent| scope.owners[parent].kind == "RadioGroup")
                    {
                        return fail(
                            b.span,
                            "RadioButton already belongs to its parent RadioGroup",
                        );
                    }
                    loop {
                        if current.repeated {
                            return fail(
                                b.span,
                                "ButtonGroup membership requires a stable element path",
                            );
                        }
                        let Some(parent) = current.parent.as_deref() else {
                            break;
                        };
                        current = &scope.owners[parent];
                    }
                    if !b.two_way
                        && let ExprKind::Reference(reference) = &b.value.kind
                        && let Some(group) = scope
                            .owners
                            .get(reference)
                            .filter(|target| target.kind == "ButtonGroup")
                    {
                        let non_exclusive = non_exclusive_group(group.members);
                        if (kind == "RadioButton") == non_exclusive {
                            return fail(
                                b.span,
                                if non_exclusive {
                                    "RadioButton requires an exclusive ButtonGroup"
                                } else {
                                    "CheckBox and Switch require a non-exclusive ButtonGroup"
                                },
                            );
                        }
                        if non_exclusive {
                            if members.iter().any(|member| matches!(member, Member::Binding(binding) if binding.name == "check-state" && !binding.two_way)) {
                                return fail(b.span, "ButtonGroup members require writable check-state; use a two-way binding");
                            }
                            let property = if kind == "CheckBox" && members.iter().any(|member| matches!(member, Member::Binding(binding) if matches!(binding.name.as_str(), "check-state" | "tristate"))) { "check-state" } else { "checked" };
                            lowered.push(LoweredMember::CheckGroupRef {
                                group: reference.clone(),
                                property,
                                span: b.span,
                            });
                        } else {
                            lowered.push(LoweredMember::RadioGroupRef {
                                group: reference.clone(),
                                span: b.span,
                            });
                        }
                        continue;
                    }
                    return fail(
                        b.span,
                        "group requires the id of a ButtonGroup in this component",
                    );
                }
                if has_action && matches!(b.name.as_str(), "checked" | "checkable" | "shortcut") {
                    return fail(
                        b.span,
                        "checked, checkable and shortcut belong to the linked Action",
                    );
                }
                if let Some(error) = control_binding_conflict(kind, |name| bindings.contains(name))
                {
                    return fail(b.span, error);
                }
                if b.two_way {
                    if instance {
                        let declaration = scope.owners[owner].properties.get(b.name.as_str())
                            .filter(|property| property.visibility == "in-out")
                            .ok_or_else(|| Diagnostic {
                                span: b.span,
                                message: format!("two-way binding requires an in-out component property `{kind}.{}`", b.name),
                            })?;
                        scope.reference(b.span, &b.name, 0, declaration.span, &b.name);
                        let ExprKind::Reference(reference) = &b.value.kind else {
                            return fail(b.span, "two-way binding requires a property reference");
                        };
                        let head = reference.split('.').next().unwrap();
                        let value = if scope.parameters.contains_key(head) {
                            let Some(repeat) = scope
                                .repeat_source
                                .as_ref()
                                .filter(|repeat| repeat.item == head)
                            else {
                                return fail(
                                    b.span,
                                    "only the repeated item or one of its fields can be linked",
                                );
                            };
                            let Some((model, span)) = &repeat.property else {
                                return fail(
                                    b.span,
                                    "delegate links require a writable model property",
                                );
                            };
                            let (_, model_type) =
                                scope.property(&repeat.owner, model, true, *span)?;
                            if model_type.list_item().is_none() {
                                return fail(
                                    b.span,
                                    "delegate links require a writable string or record model",
                                );
                            }
                            let value = expression(&b.value, scope, owner)?;
                            if !is_record_projection(&value, head) {
                                return fail(
                                    b.span,
                                    "delegate links require a writable model field",
                                );
                            }
                            value
                        } else {
                            let (target, ty) =
                                scope.property(owner, reference, true, b.value.span)?;
                            ir::Expr::read(target, ty, b.value.span)
                        };
                        if value.ty != declaration.ty {
                            return fail(
                                b.value.span,
                                format!(
                                    "two-way binding requires a writable {} property or model field, found {}",
                                    declaration.ty.name(),
                                    value.ty.name()
                                ),
                            );
                        }
                        lowered.push(LoweredMember::InstanceLink(ir::Binding {
                            name: b.name.clone(),
                            value,
                            span: b.span,
                        }));
                        continue;
                    }
                    let field_type = match (kind, b.name.as_str()) {
                        ("TableCell" | "TreeCell", "text") => Some(Type::String),
                        _ if scope.row_template.is_some() => row_editor_type(kind, &b.name),
                        _ => None,
                    };
                    if let Some(field_type) = field_type {
                        let Some(item) = scope.writable_row.as_deref().filter(|item| {
                            matches!(&b.value.kind, ExprKind::Reference(reference)
                                if reference.split('.').next() == Some(*item)
                                    && (kind == "TableCell" || reference.contains('.')))
                        }) else {
                            let message = if kind == "TableCell" {
                                "editable TableCell text requires the repeated item of a writable [string] property or a string field of a writable record list"
                            } else if kind == "TreeCell" {
                                "editable TreeCell text requires a string field of a writable record list"
                            } else if scope.row_template == Some(RowTemplate::Tree) {
                                "tree editor binding requires a matching field of a writable record list"
                            } else if scope.row_template == Some(RowTemplate::List) {
                                "a virtual toggle delegate requires a matching field of a writable record list"
                            } else {
                                "table editor binding requires a matching field of a writable record list"
                            };
                            return fail(b.span, message);
                        };
                        let value = expression(&b.value, scope, owner)?;
                        if !is_record_projection(&value, item) {
                            return fail(b.span, "row editor requires a writable model field");
                        }
                        if value.ty != field_type {
                            let editor = if kind == "TreeCell" {
                                "TreeCell"
                            } else if scope.row_template == Some(RowTemplate::Tree) {
                                kind
                            } else {
                                "table"
                            };
                            return fail(
                                b.value.span,
                                format!(
                                    "type mismatch: {editor} editor expects {}, found {}",
                                    field_type.name(),
                                    value.ty.name()
                                ),
                            );
                        }
                        lowered.push(LoweredMember::RowLink(ir::Binding {
                            name: b.name.clone(),
                            value,
                            span: b.span,
                        }));
                        continue;
                    }
                    let expected = match (kind, b.name.as_str()) {
                        _ if viewport_offset => Type::Length,
                        (kind, "text") if text_input(kind) => Type::String,
                        ("CheckBox" | "Switch", "checked") => Type::Bool,
                        ("CheckBox", "check-state") => Type::CheckState,
                        ("MenuItem" | "Action" | "DelayButton" | "GroupBox", "checked") => {
                            Type::Bool
                        }
                        (kind, "open")
                            if matches!(kind, "Dialog" | "Drawer" | "PopupWindow")
                                || picker_popup(kind) =>
                        {
                            Type::Bool
                        }
                        ("TableView" | "StandardTableView", "current-row") => Type::Int,
                        ("TreeView", "current-key") => Type::Int,
                        ("SwipeDelegate", "open-side") => Type::SwipeSide,
                        ("Timer", "running") => Type::Bool,
                        ("SecondaryWindow", "visible") => Type::Bool,
                        ("PageIndicator" | "Tumbler", "current-index") => Type::Int,
                        ("Calendar" | "MonthGrid", "date" | "display-month")
                        | ("DatePickerPopup", "date") => Type::Date,
                        ("TimePicker" | "TimePickerPopup", "time") => Type::Time,
                        ("ColorPickerPopup", "color") => Type::Color,
                        ("FontPickerPopup", "font") => Type::Font,
                        ("Slider" | "Dial" | "DoubleSpinBox", "value") => Type::Float,
                        ("ScrollBar", "position") => Type::Float,
                        ("RangeSlider", "lower-value" | "upper-value") => Type::Float,
                        ("SpinBox", "value")
                        | (
                            "ComboBox" | "RadioGroup" | "ButtonGroup" | "TabWidget" | "TabBar"
                            | "SwipeView" | "ListView" | "GridView" | "StandardListView",
                            "current-index",
                        ) => Type::Int,
                        ("ComboBox", "current-value") => combo
                            .as_ref()
                            .map_or(Type::String, |combo| combo.value_type.clone()),
                        ("RadioGroup" | "ButtonGroup", "current-value") => Type::String,
                        ("ActionGroup", "current-value") => Type::String,
                        ("ComboBox", "edit-text") => Type::String,
                        _ => {
                            return fail(
                                b.span,
                                "two-way binding requires an editable control property",
                            );
                        }
                    };
                    let ExprKind::Reference(reference) = &b.value.kind else {
                        return fail(b.span, "two-way binding requires a property reference");
                    };
                    if kind == "CheckBox"
                        && b.name == "check-state"
                        && let Some(value) = scope.check_group_read(reference, b.span)?
                    {
                        if members.iter().any(|member| matches!(member, Member::Binding(binding) if binding.name == "tristate")) {
                            return fail(b.span, "an aggregate CheckBox uses the group's three-state value without tristate");
                        }
                        lowered.push(LoweredMember::ControlLink(ir::Binding {
                            name: b.name.clone(),
                            value,
                            span: b.span,
                        }));
                        continue;
                    }
                    let (target, ty) = scope.property(owner, reference, true, b.value.span)?;
                    if viewport_offset
                        && viewport_target.replace(target.clone()).as_ref() == Some(&target)
                    {
                        return fail(b.span, "viewport axes require distinct writable properties");
                    }
                    if kind == "RangeSlider"
                        && range_target.replace(target.clone()).as_ref() == Some(&target)
                    {
                        return fail(
                            b.span,
                            "RangeSlider endpoints require distinct writable properties",
                        );
                    }
                    if matches!(kind, "Calendar" | "MonthGrid")
                        && calendar_target.replace(target.clone()).as_ref() == Some(&target)
                    {
                        return fail(
                            b.span,
                            "Calendar date and display-month require distinct writable properties",
                        );
                    }
                    if kind == "ComboBox"
                        && matches!(b.name.as_str(), "current-value" | "edit-text")
                        && combo_text_target.replace(target.clone()).as_ref() == Some(&target)
                    {
                        return fail(
                            b.span,
                            "ComboBox edit-text and current-value require distinct writable properties",
                        );
                    }
                    if ty != expected {
                        return fail(
                            b.span,
                            format!(
                                "two-way binding requires a writable {} property",
                                expected.name()
                            ),
                        );
                    }
                    lowered.push(LoweredMember::ControlLink(ir::Binding {
                        name: b.name.clone(),
                        value: ir::Expr::read(target, ty, b.value.span),
                        span: b.span,
                    }));
                    continue;
                }
                if b.name == "model"
                    && let Some(combo) = &combo
                {
                    lowered.push(LoweredMember::Binding(ir::Binding {
                        name: b.name.clone(),
                        value: combo.model.clone(),
                        span: b.span,
                    }));
                    continue;
                }
                if kind == "StandardTableView" && b.name == "model" {
                    let value = expression(&b.value, scope, owner)?;
                    let Type::RecordList(name) = &value.ty else {
                        return fail(b.span, "StandardTableView model requires a record list");
                    };
                    let mut editable = false;
                    for member in members {
                        if let Member::Binding(field) = member
                            && field.name == "row-header-field"
                        {
                            check_table_display_field(
                                field,
                                &scope.records,
                                name,
                                "row-header-field",
                            )?;
                        }
                        let Member::Element(column) = member else {
                            continue;
                        };
                        if column.ty != "TableColumn" {
                            continue;
                        }
                        check_standard_column(column, &scope.records, name)?;
                        editable |= column.members.iter().any(|member| {
                            matches!(member, Member::Binding(binding) if binding.name == "editable")
                        });
                    }
                    if editable {
                        let ExprKind::Reference(reference) = &b.value.kind else {
                            return fail(
                                b.span,
                                "editable columns require a writable model property",
                            );
                        };
                        scope.property(owner, reference, true, b.value.span)?;
                    }
                    lowered.push(LoweredMember::Binding(ir::Binding {
                        name: b.name.clone(),
                        value,
                        span: b.span,
                    }));
                    continue;
                }
                if kind == "StandardListView" && b.name == "model" {
                    let value = expression(&b.value, scope, owner)?;
                    match &value.ty {
                        Type::StringList if text_role.is_none() => {}
                        Type::RecordList(name) => {
                            let field = text_role.as_deref().unwrap_or("text");
                            if record_field_type(&scope.records, name, field) != Some(Type::String)
                            {
                                return fail(
                                    b.span,
                                    format!(
                                        "StandardListView model requires a string field named `{field}`"
                                    ),
                                );
                            }
                        }
                        Type::StringList => {
                            return fail(
                                b.span,
                                "StandardListView text-role requires a record model",
                            );
                        }
                        _ => {
                            return fail(
                                b.span,
                                "StandardListView model requires [string] or a record list",
                            );
                        }
                    }
                    lowered.push(LoweredMember::Binding(ir::Binding {
                        name: b.name.clone(),
                        value,
                        span: b.span,
                    }));
                    continue;
                }
                let placement = if instance && !secondary_content {
                    layout_item_property(&b.name)
                } else {
                    None
                };
                let ty = if b.name == "current-value"
                    && let Some(combo) = &combo
                {
                    combo.value_type.clone()
                } else if let Some(ty) = &placement {
                    ty.clone()
                } else if instance {
                    let (_, ty) =
                        scope.property(owner, &format!("self.{}", b.name), true, b.span)?;
                    if let Some(declaration) = scope.owners[owner].properties.get(b.name.as_str()) {
                        scope.reference(b.span, &b.name, 0, declaration.span, &b.name);
                    }
                    ty
                } else {
                    builtin_property(source_kind, &b.name).ok_or_else(|| Diagnostic {
                        span: b.span,
                        message: format!("unknown property `{}.{}`", kind, b.name),
                    })?
                };
                let value = if relative_size(kind, &b.name, &ty) {
                    let value = expression(&b.value, scope, owner)?;
                    if value.ty == Type::Percent {
                        value
                    } else {
                        coerce(value, ty)?
                    }
                } else {
                    expression_as(&b.value, scope, owner, ty)?
                };
                if kind == "StackView" && b.name == "initial-page" {
                    scope.check_stack_route(owner, &value)?;
                }
                if kind == "TreeRow"
                    && matches!(b.name.as_str(), "key" | "parent-key")
                    && !scope.repeat_source.as_ref().is_some_and(|repeat| {
                        value.op == "get_field" && is_record_projection(&value, &repeat.item)
                    })
                {
                    return fail(
                        b.span,
                        format!(
                            "TreeRow {} requires an integer field of the repeated record",
                            b.name
                        ),
                    );
                }
                if b.name == "cross-axis-alignment"
                    && value.op == "literal"
                    && value.value == "auto"
                {
                    return fail(b.span, "`auto` is only valid for cross-axis-self-alignment");
                }
                let binding = ir::Binding {
                    name: b.name.clone(),
                    value,
                    span: b.span,
                };
                lowered.push(if placement.is_some() {
                    LoweredMember::InstanceLayout(binding)
                } else {
                    LoweredMember::Binding(binding)
                });
            }
            Member::Element(e) => element(
                (&e.ty, e.id.as_deref(), e.span),
                &e.members,
                scope,
                &key(e, owner, index),
                e.condition.as_ref(),
                e.repeat.as_ref(),
                lowered,
            )?,
            Member::Handler(h) if clipboard_parser && h.name == "clipboard-paste" => {}
            Member::Handler(h) if instance => {
                let info = scope
                    .callback_info(owner, &h.name)
                    .ok_or_else(|| Diagnostic {
                        span: h.span,
                        message: format!("unknown callback `{kind}.{}`", h.name),
                    })?;
                if !handlers.insert(&h.name) {
                    return fail(h.span, "duplicate instance callback handler");
                }
                let mut callback = lower_callable(info, Some(h), scope, owner)?;
                callback.span = h.span;
                callback.alias = None;
                lowered.push(LoweredMember::InstanceHandler(callback));
            }
            Member::Handler(h) if scope.callback_info(owner, &h.name).is_some() => {}
            Member::Handler(h) if !root || control_event(kind, &h.name) => {
                if has_action {
                    return fail(h.span, "activation handler belongs to the linked Action");
                }
                if !builtin_event(kind, &h.name) {
                    return fail(h.span, format!("unknown event `{kind}.{}`", h.name));
                }
                let close_requested = is_window(kind) && h.name == "close-requested";
                let leave_requested = kind == "StackPage" && h.name == "leave-requested";
                let scrolled = matches!(h.name.as_str(), "scrolled" | "flicked");
                if close_requested && !root {
                    return fail(
                        h.span,
                        "close-requested requires the component's root Window",
                    );
                }
                if id.is_none()
                    && !virtual_delegate
                    && !matches!(kind, "MenuItem" | "Shortcut")
                    && !close_requested
                    && !(root && scrolled)
                    && !(root
                        && matches!(
                            kind,
                            "ListView"
                                | "GridView"
                                | "StandardListView"
                                | "SwipeView"
                                | "SwipeDelegate"
                                | "TabBar"
                                | "PageIndicator"
                                | "Calendar"
                                | "MonthGrid"
                                | "TimePicker"
                                | "TimePickerPopup"
                                | "DatePickerPopup"
                                | "ColorPickerPopup"
                                | "FontPickerPopup"
                                | "StackView"
                                | "TreeView"
                                | "TableView"
                                | "StandardTableView"
                                | "TextEdit"
                                | "TextArea"
                                | "Dialog"
                                | "Drawer"
                                | "SplitView"
                                | "RangeSlider"
                                | "Dial"
                                | "DoubleSpinBox"
                                | "SearchField"
                                | "Tumbler"
                                | "DelayButton"
                                | "CheckBox"
                                | "GroupBox"
                                | "FocusScope"
                                | "WheelHandler"
                                | "HoverHandler"
                                | "TouchArea"
                        ))
                {
                    return fail(
                        h.span,
                        "elements with handlers require an id (`name := Button { ... }`)",
                    );
                }
                let parameter_types = event_parameters(
                    kind,
                    &h.name,
                    combo.as_ref().map(|combo| &combo.value_type),
                    h.span,
                )?;
                if h.parameters.len() != parameter_types.len() {
                    let message = if parameter_types.is_empty() {
                        format!("event {} takes no parameters", h.name)
                    } else if kind == "RangeSlider" {
                        format!("event {} expects two float parameters", h.name)
                    } else if h.name == "current-cell-changed" {
                        "event current-cell-changed expects two int parameters".into()
                    } else if h.name == "column-moved" {
                        "event column-moved expects two int parameters".into()
                    } else if h.name == "row-moved" {
                        "event row-moved expects three int parameters".into()
                    } else if kind == "StackView" {
                        format!("event {} expects a string page and an int depth", h.name)
                    } else if leave_requested {
                        "event leave-requested expects a string destination and a bool removing flag".into()
                    } else {
                        format!(
                            "event {} expects one {} parameter",
                            h.name,
                            parameter_types[0].name()
                        )
                    };
                    return fail(h.span, message);
                }
                let parameters = h
                    .parameters
                    .iter()
                    .zip(parameter_types)
                    .map(|(name, ty)| {
                        check_name(name, h.span)?;
                        Ok(ast::Parameter {
                            name: name.clone(),
                            ty: ty.clone(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                if !handlers.insert(&h.name) {
                    return fail(h.span, "duplicate event handler");
                }
                let mut handler_scope = scope.clone();
                handler_scope.effects_allowed = true;
                if !virtual_delegate {
                    handler_scope.parameters.clear();
                    handler_scope.parameter_sites.clear();
                }
                handler_scope.record_handler_parameters(h);
                handler_scope.parameters.extend(
                    parameters
                        .iter()
                        .map(|parameter| (parameter.name.clone(), parameter.ty.clone())),
                );
                let mut body = statements(h, &handler_scope, owner)?;
                let result = if let Some(ty) = event_return_type(kind, &h.name) {
                    let Some(expr) = &h.result else {
                        let message = if ty == Type::Bool {
                            format!("{} must return a bool", h.name)
                        } else {
                            "input handlers must return EventResult.accept or EventResult.reject"
                                .into()
                        };
                        return fail(h.span, message);
                    };
                    Some(expression_as(expr, &handler_scope, owner, ty)?)
                } else {
                    if let Some(expr) = &h.result {
                        body.push(discarded_call(expr, &handler_scope, owner)?);
                    }
                    None
                };
                lowered.push(LoweredMember::Handler(ir::Callback {
                    name: h.name.clone(),
                    parameters,
                    body,
                    span: h.span,
                    result,
                    pure: false,
                    alias: None,
                    has_handler: true,
                }));
            }
            Member::Property(p) if !root => {
                lowered.push(LoweredMember::Property(property(p, scope, owner)?));
            }
            Member::Animation(animation) => {
                let target = &animation.property;
                if !animations.insert(target) {
                    return fail(
                        animation.span,
                        format!("duplicate animation for `{target}`"),
                    );
                }
                if let Some(error) = animation_target_error(kind, target, root) {
                    return fail(animation.span, error);
                }
                lowered.push(LoweredMember::Animation(lower_animation(
                    animation, scope, owner,
                )?));
            }
            _ => {}
        }
    }
    lowered.push(LoweredMember::EndElement);
    Ok(())
}

fn state_target<'a>(
    reference: &'a str,
    owner: &'a str,
    scope: &'a Scope<'_>,
    span: Span,
) -> Result<(&'a str, &'a str, Type, bool)> {
    let (target, property) = reference.split_once('.').unwrap_or((owner, reference));
    let target = scope.receiver(owner, target).unwrap_or(target);
    let Some(element) = scope.owners.get(target) else {
        return fail(span, format!("unknown state target `{target}`"));
    };
    let mut path = target;
    while path != owner {
        let current = &scope.owners[path];
        if current.repeated {
            return fail(span, "state targets cannot cross a repetition boundary");
        }
        let Some(parent) = current.parent.as_deref() else {
            return fail(
                span,
                "state targets must belong to the declaring element or its descendants",
            );
        };
        path = parent;
    }
    if !visual_state_target(&element.kind) {
        return fail(
            span,
            "state targets must be visual elements or component instances",
        );
    }
    if element.members.iter().any(|member| {
        matches!(member, Member::Binding(binding)
        if binding.name == property && binding.two_way)
    }) {
        return fail(
            span,
            format!("state cannot override two-way property `{reference}`"),
        );
    }
    let kind = element.kind.as_str();
    if element.instance {
        let secondary = element
            .parent
            .as_deref()
            .is_some_and(|parent| scope.owners[parent].kind == "SecondaryWindow");
        if !secondary && let Some(ty) = layout_item_property(property) {
            return Ok((target, property, ty, false));
        }
        let Some(input) = element.properties.get(property) else {
            return fail(span, format!("unknown component input `{reference}`"));
        };
        if !matches!(input.visibility.as_str(), "in" | "in-out") {
            return fail(
                span,
                format!("state requires a public writable component input `{reference}`"),
            );
        }
        scope.member_reference(span, reference, target, input.span, property);
        return Ok((target, property, input.ty.clone(), true));
    }
    let Some(ty) = state_property(kind, property) else {
        return fail(span, format!("state cannot override `{kind}.{property}`"));
    };
    Ok((target, property, ty, false))
}

pub fn visual_state_target(kind: &str) -> bool {
    !nonvisual_element(kind) && kind != "TableColumn"
}

pub fn state_property(kind: &str, property: &str) -> Option<Type> {
    let visual = layout_item_property(property).is_some()
        || box_shadow_type(property).is_some()
        || property == "clip"
        || kind == "RoundButton" && property == "radius"
        || kind == "ToolSeparator"
            && matches!(property, "orientation" | "stroke-width" | "padding")
        || pane(kind)
            && matches!(
                property,
                "padding" | "padding-left" | "padding-right" | "padding-top" | "padding-bottom"
            )
        || matches!(
            property,
            "x" | "y"
                | "width"
                | "height"
                | "background"
                | "color"
                | "border-width"
                | "border-color"
                | "border-radius"
                | "border-top-left-radius"
                | "border-top-right-radius"
                | "border-bottom-left-radius"
                | "border-bottom-right-radius"
                | "visible"
                | "enabled"
                | "link-color"
                | "font-family"
                | "font-weight"
                | "font-italic"
                | "font-size"
                | "accessible-name"
                | "accessible-description"
                | "title"
                | "source"
                | "source-clip-x"
                | "source-clip-y"
                | "source-clip-width"
                | "source-clip-height"
                | "horizontal-tile-mode"
                | "vertical-tile-mode"
                | "image-rendering"
                | "colorize"
                | "fill"
                | "stroke"
                | "stroke-width"
        )
        || property == "text"
            && matches!(
                kind,
                "Text"
                    | "Label"
                    | "StyledText"
                    | "Button"
                    | "ToolButton"
                    | "RoundButton"
                    | "DialogButton"
                    | "DelayButton"
                    | "StandardButton"
                    | "CheckBox"
                    | "RadioButton"
                    | "Switch"
                    | "MenuItem"
            );
    builtin_property(kind, property).filter(|_| visual)
}

fn lower_states(
    members: &[Member],
    scope: &Scope<'_>,
    owner: &str,
    span: Span,
) -> Result<Option<(ir::StateSelection, BTreeMap<String, StateTarget>)>> {
    let mut selection = ir::StateSelection {
        owner: owner.into(),
        cases: Vec::new(),
        span,
    };
    let mut targets: BTreeMap<String, StateTarget> = BTreeMap::new();
    let mut names = BTreeSet::new();
    for state in members
        .iter()
        .filter_map(|member| {
            if let Member::States(block) = member {
                Some(block.states.as_slice())
            } else {
                None
            }
        })
        .flatten()
    {
        if nonvisual_element(&scope.owners[owner].kind) {
            return fail(
                state.span,
                "states require a visual element or component instance",
            );
        }
        check_name(&state.name, state.span)?;
        if !names.insert(&state.name) {
            return fail(state.span, format!("duplicate state `{}`", state.name));
        }
        let when = state.when.as_ref().map_or_else(
            || Ok(ir::Expr::literal(Type::Bool, "false", state.span)),
            |value| expression_as(value, scope, owner, Type::Bool),
        )?;
        let case = selection.cases.len() as i64;
        selection.cases.push((state.name.clone(), when));
        let mut changes = BTreeMap::new();
        for change in &state.changes {
            let (target, name, ty, instance_input) =
                state_target(&change.name, owner, scope, change.span)?;
            let can_animate = !instance_input && animatable(&ty);
            if changes.insert((target, name), can_animate).is_some() {
                return fail(
                    change.span,
                    format!("duplicate state binding for `{}`", change.name),
                );
            }
            if scope.state_targets.get(target).is_some_and(|state| {
                state
                    .bindings
                    .iter()
                    .any(|binding| binding.binding.name == name)
            }) {
                return fail(
                    change.span,
                    format!("`{}` already belongs to another state group", change.name),
                );
            }
            let value = if !instance_input && relative_size(&scope.owners[target].kind, name, &ty) {
                let value = expression(&change.value, scope, owner)?;
                if value.ty == Type::Percent {
                    value
                } else {
                    coerce(value, ty)?
                }
            } else {
                expression_as(&change.value, scope, owner, ty)?
            };
            targets
                .entry(target.into())
                .or_default()
                .bindings
                .push(ir::StateBinding {
                    owner: owner.into(),
                    case,
                    instance_input,
                    binding: ir::Binding {
                        name: name.into(),
                        value,
                        span: change.span,
                    },
                });
        }
        let mut timing_keys = BTreeSet::new();
        for wildcard in [false, true] {
            for transition in &state.transitions {
                let directions: &[&str] = match transition.direction.as_str() {
                    "in" => &["in"],
                    "out" => &["out"],
                    "in-out" => &["in", "out"],
                    _ => return fail(state.span, "invalid transition direction"),
                };
                for animation in transition
                    .animations
                    .iter()
                    .filter(|animation| (animation.property == "*") == wildcard)
                {
                    let properties = if wildcard {
                        changes
                            .iter()
                            .filter(|(_, can_animate)| **can_animate)
                            .map(|(&key, _)| key)
                            .collect()
                    } else {
                        let (target, name, _, _) =
                            state_target(&animation.property, owner, scope, animation.span)?;
                        if changes.get(&(target, name)) != Some(&true) {
                            return fail(
                                animation.span,
                                "transitions require an animatable property changed by this state",
                            );
                        }
                        vec![(target, name)]
                    };
                    let config = lower_animation(animation, scope, owner)?;
                    for (target, property) in properties {
                        let element = &scope.owners[target];
                        if target == "root" && is_window(&element.kind) {
                            return fail(
                                animation.span,
                                "animate a visual child instead of the Window",
                            );
                        }
                        if element.members.iter().any(|member| matches!(member, Member::Animation(existing) if existing.property == property)) {
                            return fail(animation.span, format!("`{property}` already has a property animation"));
                        }
                        if !element.members.iter().any(|member| matches!(member, Member::Binding(binding) if binding.name == property)) {
                            return fail(animation.span, format!("transition for `{property}` requires a base binding"));
                        }
                        for &direction in directions {
                            if !timing_keys.insert((target, property, direction)) {
                                if wildcard {
                                    continue;
                                }
                                return fail(
                                    animation.span,
                                    format!("duplicate `{direction}` transition for `{property}`"),
                                );
                            }
                            let mut animation = config.clone();
                            animation.property = property.into();
                            targets.entry(target.into()).or_default().timings.push(
                                ir::StateTiming {
                                    owner: owner.into(),
                                    case,
                                    direction: direction.into(),
                                    animation,
                                },
                            );
                        }
                    }
                }
            }
        }
    }
    Ok((!selection.cases.is_empty()).then_some((selection, targets)))
}

/// Reject controls configured with conflicting forms of the same input.
pub fn control_binding_conflict(kind: &str, has: impl Fn(&str) -> bool) -> Option<String> {
    Some(match kind {
        "Shortcut" if has("sequence") && has("sequences") => {
            "Shortcut requires sequence or sequences".into()
        }
        "ComboBox" | "RadioGroup" | "ButtonGroup"
            if has("current-index") && has("current-value") =>
        {
            format!("{kind} selection accepts either current-index or current-value")
        }
        "CheckBox" if has("checked") && (has("check-state") || has("tristate")) => {
            "CheckBox accepts checked or check-state/tristate".into()
        }
        "Tumbler" if has("model") && has("count") => "Tumbler accepts either model or count".into(),
        _ => return None,
    })
}

/// Check a standalone animation's target, independently of its timing expression.
pub fn animation_target_error(kind: &str, target: &str, root: bool) -> Option<String> {
    if root && is_window(kind) {
        Some("animate a visual child instead of the Window".into())
    } else if kind == "AnimatedImage" && playback_property(target) {
        Some("AnimatedImage playback state is read-only".into())
    } else if kind == "MediaPlayer" && media_read_type(target).is_some() {
        Some("MediaPlayer playback state is read-only".into())
    } else if viewport_property(kind, target) || geometry_property(target).is_some() {
        Some(format!("property `{kind}.{target}` is read-only"))
    } else if !builtin_property(kind, target)
        .as_ref()
        .is_some_and(animatable)
    {
        Some(format!("cannot animate `{kind}.{target}`"))
    } else {
        None
    }
}

fn animatable(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Length | Type::Color | Type::Brush | Type::Float | Type::Int
    )
}

pub fn relative_size(kind: &str, name: &str, ty: &Type) -> bool {
    *ty == Type::Length
        && (matches!(
            name,
            "width"
                | "height"
                | "min-width"
                | "min-height"
                | "max-width"
                | "max-height"
                | "preferred-width"
                | "preferred-height"
                | "transform-origin-x"
                | "transform-origin-y"
        ) || content_viewport(kind) && matches!(name, "content-width" | "content-height"))
}

fn lower_animation(
    animation: &xrack_syntax::ast::Animation,
    scope: &Scope<'_>,
    owner: &str,
) -> Result<ir::Animation> {
    use xrack_syntax::ast::AnimationValue;
    let mut timing = ir::Animation {
        property: animation.property.clone(),
        duration_ms: 0,
        delay_ms: 0,
        curve: None,
        enabled: None,
        span: animation.span,
    };
    let mut seen = BTreeSet::new();
    for option in &animation.options {
        let name = option.name.as_str();
        if !seen.insert(name) {
            return fail(option.span, format!("duplicate animation option `{name}`"));
        }
        match (name, &option.value) {
            ("duration" | "delay", AnimationValue::Milliseconds(value)) => {
                let value = value
                    .parse::<u32>()
                    .ok()
                    .filter(|ms| *ms <= 60_000)
                    .ok_or_else(|| Diagnostic {
                        span: option.span,
                        message: format!("animation {name} must be 0–60000ms"),
                    })?;
                if name == "duration" {
                    timing.duration_ms = value;
                } else {
                    timing.delay_ms = value;
                }
            }
            ("duration" | "delay", _) => {
                return fail(
                    option.span,
                    format!("animation {name} requires an integer duration in ms"),
                );
            }
            ("easing", AnimationValue::Expression(value)) => timing.curve = easing_curve(value)?,
            ("enabled", AnimationValue::Expression(value)) => {
                timing.enabled = Some(expression_as(value, scope, owner, Type::Bool)?)
            }
            ("easing" | "enabled", _) => {
                return fail(option.span, format!("invalid animation {name}"));
            }
            _ => return fail(option.span, format!("unknown animation option `{name}`")),
        }
    }
    Ok(timing)
}

fn easing_curve(value: &xrack_syntax::ast::Expr) -> Result<Option<[f64; 4]>> {
    use xrack_syntax::ast::ExprKind;
    fn number(value: &xrack_syntax::ast::Expr) -> Option<f64> {
        let (value, sign) = match &value.kind {
            ExprKind::Unary { op, value } if op == "-" => (value.as_ref(), -1.0),
            ExprKind::Unary { op, value } if op == "+" => (value.as_ref(), 1.0),
            _ => (value, 1.0),
        };
        match &value.kind {
            ExprKind::Literal {
                value,
                ty: Type::Int | Type::Float,
            } => value
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .map(|v| v * sign),
            _ => None,
        }
    }
    match &value.kind {
        ExprKind::Reference(name) => {
            let name = name.strip_prefix("Easing.").unwrap_or(name);
            Ok(Some(match name {
                "linear" => return Ok(None),
                "ease" => [0.25, 0.1, 0.25, 1.0],
                "ease-in" => [0.42, 0.0, 1.0, 1.0],
                "ease-out" => [0.0, 0.0, 0.58, 1.0],
                "ease-in-out" => [0.42, 0.0, 0.58, 1.0],
                _ => return fail(value.span, format!("unknown easing `{name}`")),
            }))
        }
        ExprKind::Call(call) if call.callee == "cubic-bezier" => {
            if let [x1, y1, x2, y2] = call.arguments.as_slice()
                && let [Some(x1), Some(y1), Some(x2), Some(y2)] =
                    [number(x1), number(y1), number(x2), number(y2)]
                && (0.0..=1.0).contains(&x1)
                && (0.0..=1.0).contains(&x2)
            {
                return Ok(Some([x1, y1, x2, y2]));
            }
            fail(
                value.span,
                "cubic-bezier requires four finite numbers with both x coordinates in [0, 1]",
            )
        }
        _ => fail(
            value.span,
            "easing requires a named curve or cubic-bezier(x1, y1, x2, y2)",
        ),
    }
}

struct RecordCombo {
    model: ir::Expr,
    value_type: Type,
}

fn record_combo(members: &[Member], scope: &Scope<'_>, owner: &str) -> Result<Option<RecordCombo>> {
    let binding = |name| {
        members.iter().find_map(|member| match member {
            Member::Binding(binding) if binding.name == name => Some(binding),
            _ => None,
        })
    };
    let text_role = binding("text-role");
    let value_role = binding("value-role");
    let enabled_role = binding("enabled-role");
    let model = binding("model")
        .map(|model| expression(&model.value, scope, owner))
        .transpose()?;
    let Some(model) = model else {
        if let Some(role) = text_role.or(value_role).or(enabled_role) {
            return fail(role.span, "ComboBox roles require a record model");
        }
        return Ok(None);
    };
    let Type::RecordList(name) = &model.ty else {
        if let Some(role) = text_role.or(value_role).or(enabled_role) {
            return fail(role.span, "ComboBox roles require a record model");
        }
        if model.ty != Type::StringList {
            return fail(model.span, "ComboBox requires [string] or a record list");
        }
        return Ok(None);
    };
    let role_name = |role: &ast::Binding| {
        if let ExprKind::Literal {
            value,
            ty: Type::String,
        } = &role.value.kind
            && !role.two_way
        {
            return serde_json::from_str::<String>(value).map_err(|error| Diagnostic {
                span: role.span,
                message: error.to_string(),
            });
        }
        fail(
            role.span,
            format!(
                "ComboBox {} requires a constant string field name",
                role.name
            ),
        )
    };
    let text = text_role
        .map(role_name)
        .transpose()?
        .unwrap_or_else(|| "text".into());
    let value = value_role
        .map(role_name)
        .transpose()?
        .unwrap_or_else(|| text.clone());
    if let Some(role) = enabled_role {
        let field = role_name(role)?;
        if record_field_type(&scope.records, name, &field) != Some(Type::Bool) {
            return fail(
                role.span,
                format!("ComboBox enabled-role requires a bool field named `{field}`"),
            );
        }
    }
    if record_field_type(&scope.records, name, &text) != Some(Type::String) {
        return fail(
            model.span,
            format!("ComboBox text-role requires a string field named `{text}`"),
        );
    }
    let Some(value_type) = record_field_type(&scope.records, name, &value) else {
        return fail(
            model.span,
            format!("ComboBox value-role has no record field named `{value}`"),
        );
    };
    if !matches!(
        value_type,
        Type::String | Type::Int | Type::Float | Type::Bool | Type::Date | Type::Time
    ) {
        return fail(
            model.span,
            "ComboBox value-role requires a string, int, float, bool, Date or Time field",
        );
    }
    Ok(Some(RecordCombo { model, value_type }))
}

fn standard_list_role(members: &[Member]) -> Result<Option<String>> {
    let Some(role) = members.iter().find_map(|member| match member {
        Member::Binding(binding) if binding.name == "text-role" => Some(binding),
        _ => None,
    }) else {
        return Ok(None);
    };
    if !members
        .iter()
        .any(|member| matches!(member, Member::Binding(binding) if binding.name == "model"))
    {
        return fail(
            role.span,
            "StandardListView text-role requires a record model",
        );
    }
    match &role.value.kind {
        ExprKind::Literal {
            value,
            ty: Type::String,
        } if !role.two_way => serde_json::from_str(value)
            .map(Some)
            .map_err(|error| Diagnostic {
                span: role.span,
                message: error.to_string(),
            }),
        _ => fail(
            role.span,
            "StandardListView text-role requires a constant string field name",
        ),
    }
}

fn check_view_record_field(
    kind: &str,
    members: &[Member],
    scope: &Scope<'_>,
    owner: &str,
    property: &str,
    expected: Type,
) -> Result<()> {
    let Some(role) = members.iter().find_map(|member| match member {
        Member::Binding(binding) if binding.name == property => Some(binding),
        _ => None,
    }) else {
        return Ok(());
    };
    let ExprKind::Literal {
        value,
        ty: Type::String,
    } = &role.value.kind
    else {
        return fail(
            role.span,
            format!("{property} requires a constant string field name"),
        );
    };
    if role.two_way {
        return fail(
            role.span,
            format!("{property} requires a constant string field name"),
        );
    }
    let name: String = serde_json::from_str(value).map_err(|error| Diagnostic {
        span: role.span,
        message: error.to_string(),
    })?;
    let model = if matches!(kind, "StandardListView" | "StandardTableView") {
        members.iter().find_map(|member| match member {
            Member::Binding(binding) if binding.name == "model" => Some(&binding.value),
            _ => None,
        })
    } else {
        members.iter().find_map(|member| match member {
            Member::Element(element) => element.repeat.as_ref().map(|repeat| &repeat.model),
            _ => None,
        })
    };
    let Some(model) = model else {
        return fail(role.span, format!("{property} requires a record model"));
    };
    let Type::RecordList(record) = expression(model, scope, owner)?.ty else {
        return fail(role.span, format!("{property} requires a record model"));
    };
    if record_field_type(&scope.records, &record, &name) != Some(expected.clone()) {
        return fail(
            role.span,
            format!(
                "{property} requires a {} field named `{name}`",
                expected.name()
            ),
        );
    }
    Ok(())
}

fn check_standard_column(
    column: &ast::Element,
    records: &BTreeMap<&str, &ast::RecordDef>,
    record: &str,
) -> Result<()> {
    let Some(field) = column.members.iter().find_map(|member| match member {
        Member::Binding(binding) if binding.name == "field" => Some(binding),
        _ => None,
    }) else {
        return fail(column.span, "StandardTableView column requires a field");
    };
    check_table_display_field(field, records, record, "TableColumn.field")
}

fn check_table_display_field(
    field: &ast::Binding,
    records: &BTreeMap<&str, &ast::RecordDef>,
    record: &str,
    role: &str,
) -> Result<()> {
    let ExprKind::Literal {
        value,
        ty: Type::String,
    } = &field.value.kind
    else {
        return fail(
            field.span,
            format!("{role} requires a constant string field name"),
        );
    };
    let name: String = serde_json::from_str(value).map_err(|error| Diagnostic {
        span: field.span,
        message: error.to_string(),
    })?;
    let Some(field_type) = record_field_type(records, record, &name) else {
        return fail(
            field.span,
            format!("record `{record}` has no field `{name}`"),
        );
    };
    if !matches!(
        field_type,
        Type::String | Type::Int | Type::Float | Type::Bool | Type::Date | Type::Time
    ) {
        return fail(
            field.span,
            format!("{role} `{name}` requires a string, int, float, bool, Date or Time field"),
        );
    }
    Ok(())
}

fn is_record_projection(mut value: &ir::Expr, item: &str) -> bool {
    while value.op == "get_field" {
        value = &value.args[0];
    }
    value.op == "param" && value.value == item
}

fn row_editor_binding(kind: &str) -> Option<(&'static str, Type)> {
    match kind {
        "SpinBox" => Some(("value", Type::Int)),
        "Slider" | "Dial" | "DoubleSpinBox" => Some(("value", Type::Float)),
        "CheckBox" | "Switch" => Some(("checked", Type::Bool)),
        _ => None,
    }
}

pub fn day_cell_inputs() -> [(&'static str, Type); 9] {
    [
        ("cell-date", Type::Date),
        ("cell-index", Type::Int),
        ("cell-weekday", Type::Int),
        ("cell-week", Type::Int),
        ("cell-week-year", Type::Int),
        ("cell-selected", Type::Bool),
        ("cell-today", Type::Bool),
        ("cell-in-month", Type::Bool),
        ("cell-enabled", Type::Bool),
    ]
}

pub fn split_handle_inputs() -> [(&'static str, Type); 4] {
    [
        ("handle-index", Type::Int),
        ("handle-orientation", Type::Orientation),
        ("handle-hovered", Type::Bool),
        ("handle-pressed", Type::Bool),
    ]
}

fn validate_visual_template(kind: &str, members: &[Member]) -> Result<()> {
    for member in members {
        match member {
            Member::Binding(binding) if !binding.two_way => {}
            Member::Element(element)
                if element.id.is_none()
                    && element.repeat.is_none()
                    && matches!(
                        element.ty.as_str(),
                        "Rectangle"
                            | "Text"
                            | "Label"
                            | "Image"
                            | "Path"
                            | "StyledText"
                            | "GridLayout"
                            | "Row"
                            | "FlowLayout"
                            | "FlexboxLayout"
                            | "VerticalBox"
                            | "HorizontalBox"
                            | "VerticalLayout"
                            | "HorizontalLayout"
                    ) =>
            {
                validate_visual_template(kind, &element.members)?;
            }
            _ => {
                return fail(
                    member.span(),
                    format!("{kind} accepts visual elements and read-only bindings"),
                );
            }
        }
    }
    Ok(())
}

fn row_editor_type(kind: &str, name: &str) -> Option<Type> {
    if (kind, name) == ("CheckBox", "check-state") {
        Some(Type::CheckState)
    } else {
        row_editor_binding(kind)
            .filter(|(property, _)| *property == name)
            .map(|(_, ty)| ty)
    }
}

fn validate_template(
    kind: &str,
    id: Option<&str>,
    members: &[Member],
    span: Span,
    tree: bool,
    allow_delegate: bool,
) -> Result<()> {
    if allow_delegate && !is_builtin(kind) {
        if id.is_some()
            || members
                .iter()
                .any(|member| !matches!(member, Member::Binding(_) | Member::Handler(_)))
        {
            return fail(
                span,
                "row component delegates require an anonymous instance with inputs and callback handlers",
            );
        }
        return Ok(());
    }
    if kind == "TreeCell" && !tree {
        return fail(
            span,
            "TreeCell requires a stable child path within a TreeRow",
        );
    }
    if id.is_some()
        || !is_builtin(kind)
        || text_input(kind)
        || matches!(
            kind,
            "Window"
                | "ApplicationWindow"
                | "Button"
                | "StandardButton"
                | "ToolButton"
                | "RoundButton"
                | "DelayButton"
                | "DialogButton"
                | "Dialog"
                | "Drawer"
                | "PopupWindow"
                | "MenuBar"
                | "Menu"
                | "MenuItem"
                | "MenuSeparator"
                | "Shortcut"
                | "ContextMenuArea"
                | "TouchArea"
                | "ScaleRotateGestureHandler"
                | "SwipeGestureHandler"
                | "WheelHandler"
                | "HoverHandler"
                | "CheckBox"
                | "Switch"
                | "Slider"
                | "ScrollBar"
                | "ScrollIndicator"
                | "Dial"
                | "RangeSlider"
                | "SpinBox"
                | "DoubleSpinBox"
                | "ComboBox"
                | "RadioGroup"
                | "TabWidget"
                | "Tab"
                | "TabBar"
                | "SwipeView"
                | "SwipeDelegate"
                | "PageIndicator"
                | "Calendar"
                | "MonthGrid"
                | "DayOfWeekRow"
                | "WeekNumberColumn"
                | "TimePicker"
                | "TimePickerPopup"
                | "DatePickerPopup"
                | "ColorPickerPopup"
                | "FontPickerPopup"
                | "Tumbler"
                | "StackView"
                | "ListView"
                | "GridView"
                | "StandardListView"
                | "TreeView"
                | "TableView"
                | "StandardTableView"
                | "TableColumn"
                | "SplitView"
                | "Row"
        )
    {
        return fail(span, "repeat templates require anonymous display elements");
    }
    for member in members {
        match member {
            Member::Binding(_) | Member::Animation(_) | Member::States(_) => {}
            Member::Element(child) => {
                if (kind == "TableRow" || tree)
                    && let Some((property, _)) = row_editor_binding(&child.ty)
                {
                    if child.id.is_some() || child.repeat.is_some()
                        || child.members.iter().any(|member| !matches!(member, Member::Binding(_) | Member::Animation(_)))
                        || !child.members.iter().any(|member| matches!(member, Member::Binding(binding) if binding.two_way && row_editor_type(&child.ty, &binding.name).is_some()))
                    {
                        let row = if tree { "tree" } else { "table" };
                        return fail(child.span, format!("{} {row} cells require an anonymous editor with `{property} <=> row.field`", child.ty));
                    }
                } else {
                    validate_template(
                        &child.ty,
                        child.id.as_deref(),
                        &child.members,
                        child.span,
                        tree && child.repeat.is_none() && child.ty != "Tooltip",
                        allow_delegate && child.repeat.is_none() && child.ty != "Tooltip",
                    )?;
                }
            }
            _ => {
                return fail(
                    member.span(),
                    "repeat templates cannot declare state or handle events",
                );
            }
        }
    }
    Ok(())
}

fn discarded_call(expr: &ast::Expr, scope: &Scope<'_>, owner: &str) -> Result<ir::Statement> {
    match &expr.kind {
        ExprKind::Call(call) => scope.effect_call(call, owner),
        _ => fail(expr.span, "this handler does not declare a return type"),
    }
}

fn statements(h: &ast::Handler, scope: &Scope<'_>, owner: &str) -> Result<Vec<ir::Statement>> {
    h.statements
        .iter()
        .map(|statement| {
            let s = match statement {
                ast::Statement::Call(call) => {
                    return scope.effect_call(call, owner);
                }
                ast::Statement::Assignment(assignment) => assignment,
            };
            if !scope.effects_allowed {
                return fail(s.span, "assignment is not allowed in a pure callback");
            }
            if !s.target.contains('.') && scope.parameters.contains_key(&s.target) {
                return fail(s.span, "callback parameters cannot be assigned");
            }
            let (target, ty) = scope.property(owner, &s.target, true, s.span)?;
            let mut value = if s.op == "=" {
                expression_as(&s.value, scope, owner, ty.clone())?
            } else {
                expression(&s.value, scope, owner)?
            };
            if s.op != "=" {
                let left = ir::Expr::read(target.clone(), ty.clone(), s.span);
                value = binary(&s.op[..1], left, value, s.span)?;
            }
            Ok(ir::Statement::Assignment(ir::Assignment {
                target,
                value: coerce(value, ty)?,
                span: s.span,
            }))
        })
        .collect()
}

const FONT_FIELDS: &[(&str, Type)] = &[
    ("family", Type::String),
    ("size", Type::Length),
    ("weight", Type::Int),
    ("italic", Type::Bool),
];

fn default(ty: Type, span: Span) -> ir::Expr {
    if ty == Type::Font {
        let fields = FONT_FIELDS
            .iter()
            .zip([r#""sans-serif""#, "16", "400", "false"])
            .map(|((_, ty), value)| ir::Expr::literal(ty.clone(), value, span))
            .collect();
        return ir::Expr::operation("make_font", ty, fields, span);
    }
    let civil = match ty {
        Type::Date => Some(("make_date", ["1970", "1", "1"])),
        Type::Time => Some(("make_time", ["0", "0", "0"])),
        _ => None,
    };
    if let Some((op, parts)) = civil {
        let parts = parts
            .into_iter()
            .map(|part| ir::Expr::literal(Type::Int, part, span))
            .collect();
        return ir::Expr::operation(op, ty, parts, span);
    }
    if ty == Type::StringList {
        return ir::Expr::operation("array", ty, vec![], span);
    }
    if ty == Type::PointerButtons {
        return ir::Expr::operation("pointer_buttons", ty, vec![], span);
    }
    if matches!(ty, Type::RecordList(_)) {
        return ir::Expr::operation("record_array", ty, vec![], span);
    }
    if matches!(ty, Type::Record(_)) {
        return ir::Expr::operation("default_record", ty, vec![], span);
    }
    if ty == Type::StyledText {
        return ir::Expr::operation("markdown", ty, vec![], span);
    }
    if ty == Type::Image {
        return ir::Expr::operation("image_url", Type::Image, vec![], span);
    }
    if ty == Type::Brush {
        return ir::Expr::operation(
            "solid_brush",
            Type::Brush,
            vec![default(Type::Color, span)],
            span,
        );
    }
    let value = match &ty {
        Type::String => "\"\"",
        Type::Bool => "false",
        Type::Color => "#00000000",
        Type::LogLevel => "info",
        Type::ImageFit => "contain",
        Type::ImageTileMode => "stretch",
        Type::ImageHorizontalAlignment | Type::ImageVerticalAlignment => "center",
        Type::ImageRendering => "smooth",
        Type::LayoutAlignment => "stretch",
        Type::CrossAxisAlignment => "auto",
        Type::Orientation => "horizontal",
        Type::FlexboxLayoutDirection => "row",
        Type::FlexboxLayoutWrap => "wrap",
        Type::DialInputMode => "circular",
        Type::ScrollBarPolicy => "as-needed",
        Type::FlickDirection => "both",
        Type::FlickBoundsBehavior => "stop-at-bounds",
        Type::FlickBoundsMovement => "follow-bounds-behavior",
        Type::ScrollSnapMode => "none",
        Type::ScrollAlignment => "contain",
        Type::TextWrap => "word-wrap",
        Type::TextHorizontalAlignment => "start",
        Type::TextVerticalAlignment => "top",
        Type::TextOverflow => "clip",
        Type::InputType => "text",
        Type::CursorShape => "auto",
        Type::ColorScheme => "system",
        Type::FileDialogMode => "open-file",
        Type::MessageLevel => "info",
        Type::MessageButtons => "ok",
        Type::CheckState => "unchecked",
        Type::DialogRole => "accept",
        Type::StandardButtonKind => "ok",
        Type::SwipeSide => "closed",
        Type::DragAction => "none",
        Type::KeyEvent
        | Type::KeyModifiers
        | Type::WheelEvent
        | Type::PointerEvent
        | Type::TouchPoint
        | Type::TouchPoints => "",
        Type::PointerEventKind => "cancel",
        Type::PointerButton => "primary",
        Type::EventResult => "reject",
        Type::FocusReason => "programmatic",
        Type::DrawerEdge => "left",
        Type::PopupPlacement => "below",
        Type::SelectionTarget => "rows",
        _ => "0",
    };
    ir::Expr::literal(ty, value, span)
}

fn expression_as(
    e: &ast::Expr,
    scope: &Scope<'_>,
    owner: &str,
    expected: Type,
) -> Result<ir::Expr> {
    if let ExprKind::Conditional { condition, yes, no } = &e.kind
        && (matches!(
            expected,
            Type::Brush | Type::PointerButtons | Type::TouchPoints
        ) || !expected.enum_variants().is_empty())
    {
        let condition = expression_as(condition, scope, owner, Type::Bool)?;
        let yes = expression_as(yes, scope, owner, expected.clone())?;
        let no = expression_as(no, scope, owner, expected.clone())?;
        return Ok(ir::Expr::operation(
            "select",
            expected,
            vec![condition, yes, no],
            e.span,
        ));
    }
    if let ExprKind::Array(items) = &e.kind
        && let Type::RecordList(name) = &expected
    {
        let items = items
            .iter()
            .map(|item| expression_as(item, scope, owner, Type::Record(name.clone())))
            .collect::<Result<Vec<_>>>()?;
        return Ok(ir::Expr::operation("record_array", expected, items, e.span));
    }
    if let ExprKind::Array(items) = &e.kind
        && expected == Type::PointerButtons
    {
        let items = items
            .iter()
            .map(|item| expression_as(item, scope, owner, Type::PointerButton))
            .collect::<Result<Vec<_>>>()?;
        return Ok(ir::Expr::operation(
            "pointer_buttons",
            expected,
            items,
            e.span,
        ));
    }
    if let ExprKind::Array(items) = &e.kind
        && items.is_empty()
        && expected == Type::TouchPoints
    {
        return Ok(ir::Expr::literal(Type::TouchPoints, "", e.span));
    }
    if let ExprKind::Reference(name) = &e.kind
        && expected.enum_variants().contains(&name.as_str())
        && !scope.parameters.contains_key(name)
        && scope.property(owner, name, false, e.span).is_err()
    {
        return Ok(ir::Expr::literal(expected, name, e.span));
    }
    coerce(expression(e, scope, owner)?, expected)
}

fn coerce(e: ir::Expr, expected: Type) -> Result<ir::Expr> {
    let op = match (&e.ty, &expected) {
        (actual, expected) if actual == expected => return Ok(e),
        (Type::Int, Type::Float) => "cast",
        (Type::Color, Type::Brush) => "solid_brush",
        (Type::Percent, Type::Float) => "to_ratio",
        _ => {
            return fail(
                e.span,
                format!(
                    "type mismatch: expected {}, found {}",
                    expected.name(),
                    e.ty.name()
                ),
            );
        }
    };
    let span = e.span;
    Ok(ir::Expr::operation(op, expected, vec![e], span))
}

fn gradient_stops(
    stops: &[ast::GradientStop],
    scope: &Scope<'_>,
    owner: &str,
    span: Span,
) -> Result<Vec<ir::Expr>> {
    if stops.is_empty() {
        return fail(span, "a gradient requires at least one color stop");
    }
    let mut offsets = stops
        .iter()
        .map(|stop| {
            stop.position
                .as_deref()
                .map(|position| {
                    let offset = position
                        .parse::<f64>()
                        .ok()
                        .filter(|v| (0.0..=100.0).contains(v))
                        .ok_or_else(|| Diagnostic {
                            span: stop.span,
                            message: "gradient stop must be between 0% and 100%".into(),
                        })?;
                    Ok(offset / 100.0)
                })
                .transpose()
        })
        .collect::<Result<Vec<_>>>()?;
    offsets[0].get_or_insert(0.0);
    offsets.last_mut().unwrap().get_or_insert(1.0);
    let mut start = 0;
    for end in 1..offsets.len() {
        let Some(to) = offsets[end] else {
            continue;
        };
        let from = offsets[start].unwrap();
        if to < from {
            return fail(
                stops[end].span,
                "gradient stop positions must be nondecreasing",
            );
        }
        let step = (to - from) / (end - start) as f64;
        for (i, offset) in offsets[start + 1..end].iter_mut().enumerate() {
            *offset = Some(from + step * (i + 1) as f64);
        }
        start = end;
    }
    stops
        .iter()
        .zip(offsets)
        .map(|(stop, offset)| {
            let color = coerce(expression(&stop.color, scope, owner)?, Type::Color)?;
            let offset = ir::Expr::literal(Type::Float, offset.unwrap().to_string(), stop.span);
            Ok(ir::Expr::operation(
                "gradient_stop",
                Type::GradientStop,
                vec![color, offset],
                stop.span,
            ))
        })
        .collect()
}

fn expression(e: &ast::Expr, scope: &Scope<'_>, owner: &str) -> Result<ir::Expr> {
    let span = e.span;
    Ok(match &e.kind {
        ExprKind::Literal { value, ty } => {
            match ty {
                Type::Int => {
                    value.parse::<i64>().map_err(|_| Diagnostic {
                        span,
                        message: "integer literal is outside i64 range".into(),
                    })?;
                }
                Type::Float | Type::Length | Type::Percent => {
                    let number = value.parse::<f64>().map_err(|_| Diagnostic {
                        span,
                        message: "invalid number".into(),
                    })?;
                    if !number.is_finite() {
                        return fail(span, "non-finite numeric literal");
                    }
                }
                Type::String => {
                    serde_json::from_str::<String>(value).map_err(|e| Diagnostic {
                        span,
                        message: format!("invalid string: {e}"),
                    })?;
                }
                Type::Color if !matches!(value.len(), 4 | 5 | 7 | 9) => {
                    return fail(
                        span,
                        "colors require 3, 4, 6 or 8 hexadecimal digits (RGBA)",
                    );
                }
                _ => {}
            }
            ir::Expr::literal(ty.clone(), value.clone(), span)
        }
        ExprKind::Translation { id, arguments } => {
            let id = serde_json::from_str::<String>(id).map_err(|error| Diagnostic {
                span,
                message: format!("invalid translation ID: {error}"),
            })?;
            if !id.starts_with(|c: char| c.is_ascii_alphabetic())
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
            {
                return fail(span, format!("invalid Fluent message ID `{id}`"));
            }
            let mut names = Vec::new();
            let mut args = Vec::new();
            for argument in arguments {
                if names.contains(&argument.name.as_str()) {
                    return fail(
                        argument.value.span,
                        format!("duplicate translation argument `{}`", argument.name),
                    );
                }
                let value = expression(&argument.value, scope, owner)?;
                if !matches!(value.ty, Type::String | Type::Int | Type::Float) {
                    return fail(
                        value.span,
                        "translation arguments require string, int or float values",
                    );
                }
                names.push(argument.name.as_str());
                args.push(value);
            }
            ir::Expr {
                value: serde_json::to_string(&(id, names)).unwrap(),
                ..ir::Expr::operation("translate", Type::String, args, span)
            }
        }
        ExprKind::Markdown(markdown) => {
            let value = serde_json::from_str::<String>(markdown).map_err(|e| Diagnostic {
                span,
                message: format!("invalid Markdown string: {e}"),
            })?;
            ir::Expr {
                value,
                ..ir::Expr::operation("markdown", Type::StyledText, vec![], span)
            }
        }
        ExprKind::ImageUrl(path) | ExprKind::SlicedImageUrl { path, .. } => {
            let value = serde_json::from_str::<String>(path).map_err(|e| Diagnostic {
                span,
                message: format!("invalid image path: {e}"),
            })?;
            let source = ir::Expr {
                value,
                ..ir::Expr::operation("image_url", Type::Image, vec![], span)
            };
            if let ExprKind::SlicedImageUrl { slice, edges, .. } = &e.kind {
                if slice != "nine-slice" {
                    return fail(span, format!("unknown image modifier `{slice}`"));
                }
                if source.value.is_empty() {
                    return fail(span, "nine-slice requires a nonempty image path");
                }
                let edges = match edges.as_slice() {
                    [all] => [all, all, all, all],
                    [vertical, horizontal] => [vertical, horizontal, vertical, horizontal],
                    [top, right, bottom, left] => [top, right, bottom, left],
                    _ => return fail(span, "nine-slice expects one, two or four edges"),
                };
                let mut operands = vec![source];
                for edge in edges {
                    let value = edge.parse::<u32>().map_err(|_| Diagnostic {
                        span,
                        message: "nine-slice edges must be nonnegative integers".into(),
                    })?;
                    operands.push(ir::Expr::literal(Type::Int, value.to_string(), span));
                }
                ir::Expr::operation("nine_slice", Type::Image, operands, span)
            } else {
                source
            }
        }
        ExprKind::Array(items) => {
            let items = items
                .iter()
                .map(|item| expression(item, scope, owner))
                .collect::<Result<Vec<_>>>()?;
            let (op, ty, item_ty) = match items.first().map(|item| &item.ty) {
                Some(Type::PointerButton) => {
                    ("pointer_buttons", Type::PointerButtons, Type::PointerButton)
                }
                Some(Type::Record(name)) => (
                    "record_array",
                    Type::RecordList(name.clone()),
                    Type::Record(name.clone()),
                ),
                _ => ("array", Type::StringList, Type::String),
            };
            let items = items
                .into_iter()
                .map(|item| coerce(item, item_ty.clone()))
                .collect::<Result<_>>()?;
            ir::Expr::operation(op, ty, items, span)
        }
        ExprKind::Record { name, fields } => {
            let builtin = match name.as_str() {
                "Date" => Some((
                    "make_date",
                    Type::Date,
                    &[
                        ("year", Type::Int),
                        ("month", Type::Int),
                        ("day", Type::Int),
                    ][..],
                )),
                "Time" => Some((
                    "make_time",
                    Type::Time,
                    &[
                        ("hour", Type::Int),
                        ("minute", Type::Int),
                        ("second", Type::Int),
                    ][..],
                )),
                "Font" => Some(("make_font", Type::Font, FONT_FIELDS)),
                _ => None,
            };
            let schema = if let Some((_, _, fields)) = &builtin {
                fields.to_vec()
            } else {
                scope
                    .records
                    .get(name.as_str())
                    .ok_or_else(|| Diagnostic {
                        span,
                        message: format!("unknown record `{name}`"),
                    })?
                    .fields
                    .iter()
                    .map(|field| (field.name.as_str(), field.ty.clone()))
                    .collect()
            };
            if let Some(record) = scope.records.get(name.as_str()) {
                scope.reference(span, "", 0, record.span, &record.name);
                for value in fields {
                    if let Some(field) = record.fields.iter().find(|field| field.name == value.name)
                    {
                        scope.reference(value.span, &value.name, 0, field.span, &field.name);
                    }
                }
            }
            let mut values = BTreeMap::new();
            for field in fields {
                if values.insert(field.name.as_str(), &field.value).is_some() {
                    return fail(
                        field.value.span,
                        format!("duplicate record field `{}`", field.name),
                    );
                }
            }
            let args = schema
                .into_iter()
                .map(|(field, ty)| {
                    if name == "Time" && !values.contains_key(field) {
                        return Ok(ir::Expr::literal(Type::Int, "0", span));
                    }
                    let value = values.remove(field).ok_or_else(|| Diagnostic {
                        span,
                        message: format!("missing record field `{field}`"),
                    })?;
                    expression_as(value, scope, owner, ty)
                })
                .collect::<Result<Vec<_>>>()?;
            if let Some((unknown, value)) = values.into_iter().next() {
                return fail(value.span, format!("unknown record field `{unknown}`"));
            }
            let (op, ty) = builtin
                .map(|(op, ty, _)| (op, ty))
                .unwrap_or_else(|| ("make_record", Type::Record(name.as_str().into())));
            ir::Expr::operation(op, ty, args, span)
        }
        ExprKind::Call(call) => {
            if let Some(function) = scope.builtin_call(call)? {
                return match function.kind {
                    FunctionKind::Format => format_call(call, scope, owner),
                    FunctionKind::Math(kind) => math_call(call, scope, owner, function, kind),
                    FunctionKind::String(kind) => {
                        let args = builtin_arguments(call, scope, owner, function)?;
                        Ok(ir::Expr {
                            value: kind.name().into(),
                            ..ir::Expr::operation("string_function", kind.result(), args, span)
                        })
                    }
                    FunctionKind::ModelCount
                    | FunctionKind::ModelIsEmpty
                    | FunctionKind::ModelAt => model_call(call, scope, owner, function),
                    FunctionKind::Color(kind) => {
                        let args = builtin_arguments(call, scope, owner, function)?;
                        Ok(ir::Expr {
                            value: kind.name().into(),
                            ..ir::Expr::operation("color_function", Type::Color, args, span)
                        })
                    }
                    FunctionKind::DateIsValid
                    | FunctionKind::DateAddDays
                    | FunctionKind::DateAddMonths
                    | FunctionKind::DateWeekday => {
                        let (op, result) = match function.kind {
                            FunctionKind::DateIsValid => ("date_is_valid", Type::Bool),
                            FunctionKind::DateAddDays => ("date_add_days", Type::Date),
                            FunctionKind::DateAddMonths => ("date_add_months", Type::Date),
                            FunctionKind::DateWeekday => ("date_weekday", Type::Int),
                            _ => unreachable!(),
                        };
                        let args = builtin_arguments(call, scope, owner, function)?;
                        Ok(ir::Expr::operation(op, result, args, span))
                    }
                    FunctionKind::Locale => Ok(ir::Expr::operation(
                        "locale",
                        Type::String,
                        builtin_arguments(call, scope, owner, function)?,
                        span,
                    )),
                    FunctionKind::FormatNumber => {
                        let args = builtin_arguments(call, scope, owner, function)?;
                        if !matches!(args[0].ty, Type::Int | Type::Float) {
                            return fail(
                                args[0].span,
                                "I18n.format-number requires an int or float",
                            );
                        }
                        Ok(ir::Expr::operation(
                            "format_number",
                            Type::String,
                            args,
                            span,
                        ))
                    }
                    FunctionKind::ParseNumber => Ok(ir::Expr::operation(
                        "parse_number",
                        Type::Float,
                        builtin_arguments(call, scope, owner, function)?,
                        span,
                    )),
                    FunctionKind::Round => {
                        let [operand] = call.arguments.as_slice() else {
                            return fail(span, "Math.round expects one argument");
                        };
                        let operand = expression(operand, scope, owner)?;
                        match operand.ty {
                            Type::Int => Ok(operand),
                            Type::Float => {
                                Ok(ir::Expr::operation("round", Type::Int, vec![operand], span))
                            }
                            _ => fail(operand.span, "Math.round requires an int or float"),
                        }
                    }
                    FunctionKind::Log | FunctionKind::LogAt(_) => fail(
                        span,
                        format!(
                            "{} can only be used as a statement in an effectful handler",
                            call.callee
                        ),
                    ),
                };
            }
            if let Some(state) = scope.split_state_read(call, owner)? {
                return Ok(state);
            }
            if let Some(query) = scope.tree_query(call, owner)? {
                return Ok(query);
            }
            if let Some(query) = scope.view_index_query(call, owner)? {
                return Ok(query);
            }
            let (call, result) = scope.call(call, owner)?;
            let ty = result.ok_or_else(|| Diagnostic {
                span,
                message: format!("callback `{}` has no return value", call.name),
            })?;
            ir::Expr {
                op: "call".into(),
                value: call.name,
                owner: Some(call.owner),
                ty,
                args: call.args,
                span,
            }
        }
        ExprKind::LinearGradient { angle, stops } => {
            let degrees = angle
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| Diagnostic {
                    span,
                    message: "gradient angle must be finite".into(),
                })?;
            let mut args = vec![ir::Expr::literal(Type::Float, degrees.to_string(), span)];
            args.extend(gradient_stops(stops, scope, owner, span)?);
            ir::Expr::operation("linear_gradient", Type::Brush, args, span)
        }
        ExprKind::RadialGradient { stops } => {
            let args = gradient_stops(stops, scope, owner, span)?;
            ir::Expr::operation("radial_gradient", Type::Brush, args, span)
        }
        ExprKind::Reference(s) => {
            if let Some((namespace, member)) = s.split_once('.')
                && !scope.owners.contains_key(namespace)
                && !scope.parameters.contains_key(namespace)
                && builtins::NAMESPACES.contains(&namespace)
            {
                return match builtins::constant(namespace, member) {
                    Some(constant) => Ok(ir::Expr::literal(
                        Type::Float,
                        constant.value.to_string(),
                        span,
                    )),
                    None => fail(span, format!("unknown built-in value `{s}`")),
                };
            }
            if let Some((namespace, name)) = s.split_once('.')
                && matches!(
                    namespace,
                    "LogLevel"
                        | "ImageFit"
                        | "ImageTileMode"
                        | "ImageHorizontalAlignment"
                        | "ImageVerticalAlignment"
                        | "ImageRendering"
                        | "LayoutAlignment"
                        | "CrossAxisAlignment"
                        | "Orientation"
                        | "FlickDirection"
                        | "FlickBoundsBehavior"
                        | "FlickBoundsMovement"
                        | "FlexboxLayoutDirection"
                        | "FlexboxLayoutWrap"
                        | "DialInputMode"
                        | "ScrollBarPolicy"
                        | "ScrollSnapMode"
                        | "ScrollAlignment"
                        | "TextWrap"
                        | "TextHorizontalAlignment"
                        | "TextVerticalAlignment"
                        | "TextOverflow"
                        | "InputType"
                        | "CursorShape"
                        | "PointerEventKind"
                        | "PointerButton"
                        | "ColorScheme"
                        | "FileDialogMode"
                        | "MessageLevel"
                        | "MessageButtons"
                        | "CheckState"
                        | "DialogRole"
                        | "StandardButtonKind"
                        | "SwipeSide"
                        | "DragAction"
                        | "EventResult"
                        | "FocusReason"
                        | "DrawerEdge"
                        | "PopupPlacement"
                        | "SelectionTarget"
                )
            {
                let ty = Type::parse(namespace).unwrap();
                if !ty.enum_variants().contains(&name) {
                    return fail(span, format!("unknown {namespace} value `{name}`"));
                }
                return Ok(ir::Expr::literal(ty, name, span));
            }
            if let Some((base, field)) = s.rsplit_once('.') {
                let base_reference = ast::Expr {
                    kind: ExprKind::Reference(base.into()),
                    span,
                };
                let record = match expression(&base_reference, scope, owner) {
                    Ok(value)
                        if matches!(
                            value.ty,
                            Type::Record(_)
                                | Type::Date
                                | Type::Time
                                | Type::Font
                                | Type::Image
                                | Type::KeyEvent
                                | Type::KeyModifiers
                                | Type::WheelEvent
                                | Type::PointerEvent
                                | Type::TouchPoint
                        ) =>
                    {
                        Some(value)
                    }
                    Err(error)
                        if base.split_once('.').is_some_and(|(item, _)| {
                            matches!(scope.parameters.get(item), Some(Type::Record(_)))
                        }) =>
                    {
                        return Err(error);
                    }
                    _ => None,
                };
                if let Some(record) = record {
                    if record.ty == Type::Image {
                        if !matches!(field, "width" | "height") {
                            return fail(span, format!("image has no field `{field}`"));
                        }
                        return Ok(ir::Expr {
                            value: field.into(),
                            ..ir::Expr::operation("image_dimension", Type::Int, vec![record], span)
                        });
                    }
                    if !matches!(record.ty, Type::Record(_)) {
                        let Some((op, ty)) = record.ty.builtin_field(field) else {
                            let owner = match record.ty {
                                Type::Date => "Date".into(),
                                Type::Time => "Time".into(),
                                Type::Font => "Font".into(),
                                _ => record.ty.name(),
                            };
                            return fail(span, format!("{owner} has no field `{field}`"));
                        };
                        return Ok(ir::Expr {
                            value: field.into(),
                            ..ir::Expr::operation(op, ty, vec![record], span)
                        });
                    }
                    let Type::Record(name) = &record.ty else {
                        unreachable!()
                    };
                    let definition = scope.records[name.as_ref()]
                        .fields
                        .iter()
                        .find(|definition| definition.name == field)
                        .ok_or_else(|| Diagnostic {
                            span,
                            message: format!("record `{name}` has no field `{field}`"),
                        })?;
                    scope.reference(span, s, s.split('.').count() - 1, definition.span, field);
                    let ty = definition.ty.clone();
                    return Ok(ir::Expr {
                        value: field.into(),
                        ..ir::Expr::operation("get_field", ty, vec![record], span)
                    });
                }
            }
            if !s.contains('.')
                && let Some(ty) = scope.parameters.get(s)
            {
                if let Some(declaration) = scope.parameter_sites.get(s) {
                    scope.reference(span, s, 0, *declaration, s);
                }
                return Ok(ir::Expr {
                    op: "param".into(),
                    value: s.clone(),
                    owner: None,
                    ty: ty.clone(),
                    args: vec![],
                    span,
                });
            }
            if let Some(read) = scope
                .focus_read(owner, s, span)?
                .or(scope.viewport_read(owner, s, span)?)
                .or(scope.scroll_control_read(owner, s, span)?)
                .or(scope.geometry_read(owner, s, span)?)
                .or(scope.gesture_read(owner, s, span)?)
                .or(scope.split_resizing(owner, s, span)?)
                .or(scope.check_group_read(s, span)?)
                .or(scope.action_group_read(owner, s, span)?)
            {
                return Ok(read);
            }
            if let Some(read) = scope.table_selection_read(owner, s, span)? {
                return Ok(read);
            }
            if let Some(read) = scope.table_sort_read(owner, s, span)? {
                return Ok(read);
            }
            if let Some(read) = scope.table_history_read(owner, s, span)? {
                return Ok(read);
            }
            if let Some(read) = scope
                .stack_read(owner, s, span)
                .or_else(|| scope.image_playback_read(owner, s, span))
                .or_else(|| scope.media_read(owner, s, span))
                .or_else(|| scope.window_read(owner, s, span))
                .or_else(|| scope.native_dialog_read(owner, s, span))
            {
                return Ok(read);
            }
            let (property, ty) = scope.property(owner, s, false, span)?;
            ir::Expr::read(property, ty, span)
        }
        ExprKind::Unary { op, value } => {
            let value = expression(value, scope, owner)?;
            let valid = if op == "!" {
                value.ty == Type::Bool
            } else {
                matches!(value.ty, Type::Int | Type::Float | Type::Length)
            };
            if !valid {
                return fail(
                    span,
                    format!("operator `{op}` cannot be applied to {}", value.ty.name()),
                );
            }
            ir::Expr {
                op: "unary".into(),
                value: op.clone(),
                owner: None,
                ty: value.ty.clone(),
                args: vec![value],
                span,
            }
        }
        ExprKind::Binary { op, left, right } => binary(
            op,
            expression(left, scope, owner)?,
            expression(right, scope, owner)?,
            span,
        )?,
        ExprKind::Conditional { condition, yes, no } => {
            let condition = coerce(expression(condition, scope, owner)?, Type::Bool)?;
            let yes = expression(yes, scope, owner)?;
            let no = expression(no, scope, owner)?;
            let (yes, no) = promote(yes, no)?;
            if yes.ty != no.ty {
                return fail(span, "conditional branches must have the same type");
            }
            ir::Expr {
                op: "select".into(),
                value: "".into(),
                owner: None,
                ty: yes.ty.clone(),
                args: vec![condition, yes, no],
                span,
            }
        }
    })
}

fn builtin_arguments(
    call: &ast::Call,
    scope: &Scope<'_>,
    owner: &str,
    function: &builtins::Function,
) -> Result<Vec<ir::Expr>> {
    if call.arguments.len() != function.parameters.len() {
        return fail(
            call.span,
            format!(
                "{} expects {} arguments, found {}",
                call.callee,
                function.parameters.len(),
                call.arguments.len()
            ),
        );
    }
    call.arguments
        .iter()
        .zip(function.parameters)
        .map(|(arg, parameter)| match parameter.expected {
            Some(ty) => expression_as(arg, scope, owner, ty.clone()),
            None => expression(arg, scope, owner),
        })
        .collect()
}

fn model_call(
    call: &ast::Call,
    scope: &Scope<'_>,
    owner: &str,
    function: &builtins::Function,
) -> Result<ir::Expr> {
    let mut args = builtin_arguments(call, scope, owner, function)?;
    let row = match &args[0].ty {
        Type::StringList => Type::String,
        Type::RecordList(name) => Type::Record(name.clone()),
        _ => {
            return fail(
                args[0].span,
                format!("{} requires a string or record model", call.callee),
            );
        }
    };
    let (op, result) = match function.kind {
        FunctionKind::ModelCount => ("model_count", Type::Int),
        FunctionKind::ModelIsEmpty => ("model_is_empty", Type::Bool),
        FunctionKind::ModelAt => {
            let fallback = coerce(args.pop().unwrap(), row.clone())?;
            args.push(fallback);
            ("model_at", row)
        }
        _ => unreachable!(),
    };
    Ok(ir::Expr::operation(op, result, args, call.span))
}

fn math_call(
    call: &ast::Call,
    scope: &Scope<'_>,
    owner: &str,
    function: &builtins::Function,
    kind: MathFunction,
) -> Result<ir::Expr> {
    let mut args = builtin_arguments(call, scope, owner, function)?;
    let first = args[0].ty.clone();
    let integral = matches!(kind, MathFunction::Floor | MathFunction::Ceil);
    let ty = if matches!(first, Type::Int | Type::Float)
        && args
            .iter()
            .all(|arg| matches!(arg.ty, Type::Int | Type::Float))
    {
        if args.iter().any(|arg| arg.ty == Type::Float) {
            Type::Float
        } else {
            Type::Int
        }
    } else if !integral
        && matches!(first, Type::Length | Type::Percent)
        && args.iter().all(|arg| arg.ty == first)
    {
        first
    } else {
        return fail(
            call.span,
            format!(
                "{}: arguments must share a numeric type and unit",
                call.callee
            ),
        );
    };
    if integral && ty == Type::Int {
        return Ok(args.remove(0));
    }
    let args = args
        .into_iter()
        .map(|arg| coerce(arg, ty.clone()))
        .collect::<Result<_>>()?;
    Ok(ir::Expr {
        value: kind.name().into(),
        ..ir::Expr::operation(
            "math_function",
            if integral {
                Type::Int
            } else if kind == MathFunction::IsFinite {
                Type::Bool
            } else {
                ty
            },
            args,
            call.span,
        )
    })
}

fn format_call(call: &ast::Call, scope: &Scope<'_>, owner: &str) -> Result<ir::Expr> {
    let Some((template, values)) = call.arguments.split_first() else {
        return fail(call.span, "Strings.format expects a string literal");
    };
    let ExprKind::Literal {
        ty: Type::String,
        value,
    } = &template.kind
    else {
        return fail(
            template.span,
            "Strings.format expects a string literal as its first argument",
        );
    };
    let pattern: String = serde_json::from_str(value).map_err(|error| Diagnostic {
        span: template.span,
        message: format!("invalid format string: {error}"),
    })?;
    let literal = |text: &str| {
        ir::Expr::literal(
            Type::String,
            serde_json::to_string(text).unwrap(),
            template.span,
        )
    };
    let mut chars = pattern.chars().peekable();
    let mut values_iter = values.iter();
    let mut parts = Vec::new();
    let mut text = String::new();
    while let Some(ch) = chars.next() {
        match ch {
            '{' | '}' if chars.next_if_eq(&ch).is_some() => text.push(ch),
            '{' => {
                let precision = if chars.next_if_eq(&'}').is_some() {
                    None
                } else if chars.next_if_eq(&':').is_some() && chars.next_if_eq(&'.').is_some() {
                    let digits: String =
                        std::iter::from_fn(|| chars.next_if(|ch| ch.is_ascii_digit())).collect();
                    let precision = digits.parse::<usize>().ok().filter(|value| *value <= 20);
                    if precision.is_none() || chars.next_if_eq(&'}').is_none() {
                        return fail(
                            template.span,
                            "Strings.format precision must be between 0 and 20",
                        );
                    }
                    precision
                } else {
                    return fail(
                        template.span,
                        "Strings.format accepts {}, {:.N}, {{ and }} in a format string",
                    );
                };
                if !text.is_empty() {
                    parts.push(literal(&text));
                    text.clear();
                }
                let Some(value) = values_iter.next() else {
                    return fail(
                        template.span,
                        "Strings.format has more placeholders than arguments",
                    );
                };
                let value = expression(value, scope, owner)?;
                if let Some(precision) = precision {
                    if !matches!(value.ty, Type::Int | Type::Float) {
                        return fail(
                            value.span,
                            "Strings.format fixed precision requires an int or float",
                        );
                    }
                    parts.push(ir::Expr {
                        value: precision.to_string(),
                        ..ir::Expr::operation("format_fixed", Type::String, vec![value], call.span)
                    });
                } else {
                    parts.push(stringify(value, "Strings.format")?);
                }
            }
            '}' => {
                return fail(
                    template.span,
                    "Strings.format accepts {}, {:.N}, {{ and }} in a format string",
                );
            }
            _ => text.push(ch),
        }
    }
    if let Some(extra) = values_iter.next() {
        return fail(
            extra.span,
            "Strings.format has more arguments than placeholders",
        );
    }
    if !text.is_empty() || parts.is_empty() {
        parts.push(literal(&text));
    }
    Ok(parts
        .into_iter()
        .reduce(|left, right| ir::Expr {
            value: "+".into(),
            ..ir::Expr::operation("binary", Type::String, vec![left, right], call.span)
        })
        .expect("format includes at least one part"))
}

fn numeric(t: &Type) -> bool {
    matches!(t, Type::Int | Type::Float)
}
fn promote(a: ir::Expr, b: ir::Expr) -> Result<(ir::Expr, ir::Expr)> {
    match (&a.ty, &b.ty) {
        (Type::Float, Type::Int) => Ok((a, coerce(b, Type::Float)?)),
        (Type::Int, Type::Float) => Ok((coerce(a, Type::Float)?, b)),
        (Type::Brush, Type::Color) => Ok((a, coerce(b, Type::Brush)?)),
        (Type::Color, Type::Brush) => Ok((coerce(a, Type::Brush)?, b)),
        _ => Ok((a, b)),
    }
}

fn stringify(e: ir::Expr, operation: &str) -> Result<ir::Expr> {
    match e.ty {
        Type::String => Ok(e),
        Type::Int | Type::Float | Type::Bool | Type::Date | Type::Time => {
            let span = e.span;
            Ok(ir::Expr::operation(
                "stringify",
                Type::String,
                vec![e],
                span,
            ))
        }
        _ => fail(
            e.span,
            format!("{operation} accepts string, int, float, bool, Date or Time"),
        ),
    }
}

fn binary(op: &str, a: ir::Expr, b: ir::Expr, span: Span) -> Result<ir::Expr> {
    if op == "+" && (a.ty == Type::String || b.ty == Type::String) {
        return Ok(ir::Expr {
            op: "binary".into(),
            value: op.into(),
            owner: None,
            ty: Type::String,
            args: vec![
                stringify(a, "string concatenation")?,
                stringify(b, "string concatenation")?,
            ],
            span,
        });
    }
    let (mut a, mut b) = promote(a, b)?;
    let ty = match op {
        "&&" | "||" if a.ty == Type::Bool && b.ty == Type::Bool => Type::Bool,
        "==" | "!=" if a.ty == b.ty => Type::Bool,
        "<" | "<=" | ">" | ">="
            if a.ty == b.ty
                && (numeric(&a.ty) || matches!(a.ty, Type::Length | Type::Date | Type::Time)) =>
        {
            Type::Bool
        }
        "+" if a.ty == Type::String && b.ty == Type::String => Type::String,
        "+" | "-" if a.ty == Type::Length && b.ty == Type::Length => Type::Length,
        "*" | "/" if a.ty == Type::Length && numeric(&b.ty) => {
            b = coerce(b, Type::Float)?;
            Type::Length
        }
        "*" if numeric(&a.ty) && b.ty == Type::Length => {
            a = coerce(a, Type::Float)?;
            Type::Length
        }
        "/" if a.ty == Type::Length && b.ty == Type::Length => Type::Float,
        "+" | "-" | "*" | "/" if numeric(&a.ty) && a.ty == b.ty => a.ty.clone(),
        "%" if a.ty == Type::Int && b.ty == Type::Int => Type::Int,
        _ => {
            return fail(
                span,
                format!(
                    "invalid operands to `{op}`: {} and {}",
                    a.ty.name(),
                    b.ty.name()
                ),
            );
        }
    };
    Ok(ir::Expr {
        op: "binary".into(),
        value: op.into(),
        owner: None,
        ty,
        args: vec![a, b],
        span,
    })
}

type Dependencies = BTreeMap<String, (Span, Vec<String>)>;

fn check_cycles(edges: &Dependencies, description: &str) -> Result<()> {
    if let Some(name) = dependency_cycle(edges) {
        return fail(
            edges[name].0,
            format!("cyclic {description} involving `{name}`"),
        );
    }
    Ok(())
}

/// Find a member that participates in a dependency cycle.
pub fn dependency_cycle<T>(edges: &BTreeMap<String, (T, Vec<String>)>) -> Option<&str> {
    fn visit<'a, T>(
        name: &'a str,
        edges: &'a BTreeMap<String, (T, Vec<String>)>,
        active: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> Option<&'a str> {
        if done.contains(name) {
            return None;
        }
        let (_, next) = edges.get(name)?;
        if !active.insert(name) {
            return Some(name);
        }
        for dependency in next {
            if let Some(name) = visit(dependency, edges, active, done) {
                return Some(name);
            }
        }
        active.remove(name);
        done.insert(name);
        None
    }

    let mut done = BTreeSet::new();
    let mut active = BTreeSet::new();
    for name in edges.keys() {
        if let Some(name) = visit(name, edges, &mut active, &mut done) {
            return Some(name);
        }
    }
    None
}

/// Resolve a global's local or cross-global dependency to a module-wide name.
pub fn qualify_global_dependency(component: &str, path: &str) -> String {
    path.strip_prefix("__global_").map_or_else(
        || format!("{component}.{path}"),
        |name| {
            let (global, member) = name.split_once('.').unwrap_or((name, ""));
            format!("{global}.root.{member}")
        },
    )
}

fn check_composition(components: &[(&ast::Component, Vec<LoweredMember<'_>>)]) -> Result<()> {
    let definitions: BTreeMap<_, _> = components
        .iter()
        .map(|(c, members)| (c.name.as_str(), members.as_slice()))
        .collect();
    let types = components
        .iter()
        .map(|(c, members)| {
            let instances = members
                .iter()
                .filter_map(|m| match m {
                    LoweredMember::BeginElement {
                        kind,
                        instance: true,
                        ..
                    } => Some((*kind).into()),
                    _ => None,
                })
                .collect();
            (c.name.clone(), (c.span, instances))
        })
        .collect();
    check_cycles(&types, "component instantiation")?;
    let mut globals = Dependencies::new();
    for (component, members) in components.iter().filter(|(component, _)| component.global) {
        let mut edges = Dependencies::new();
        let mut links = BTreeMap::new();
        collect_bindings(
            members,
            &definitions,
            "",
            &mut edges,
            &mut links,
            &mut GeometryFlow::default(),
            false,
        );
        for (key, (span, dependencies)) in edges {
            globals.insert(
                qualify_global_dependency(&component.name, &key),
                (
                    span,
                    dependencies
                        .iter()
                        .map(|name| qualify_global_dependency(&component.name, name))
                        .collect(),
                ),
            );
        }
    }
    check_cycles(&globals, "global binding or callback call")?;
    for members in definitions.values() {
        let mut edges = Dependencies::new();
        let mut links = BTreeMap::new();
        let mut geometry = GeometryFlow::default();
        collect_bindings(
            members,
            &definitions,
            "",
            &mut edges,
            &mut links,
            &mut geometry,
            false,
        );
        let overlays: Vec<_> = edges
            .extract_if(.., |key, _| key.starts_with("@state-input:"))
            .collect();
        for (key, (span, dependencies)) in overlays {
            let destination = key.strip_prefix("@state-input:").unwrap();
            if links.contains_key(destination) {
                return fail(span, "state cannot override a two-way component input");
            }
            edges
                .entry(destination.into())
                .or_insert((span, Vec::new()))
                .1
                .extend(dependencies);
        }
        check_cycles(&edges, "property binding or callback call")?;
        geometry.validate(&edges)?;
    }
    Ok(())
}

#[derive(Default)]
struct GeometryFlow {
    overlays: BTreeMap<String, bool>,
    sinks: Vec<(String, String, Span)>,
}

impl GeometryFlow {
    fn validate(&self, edges: &Dependencies) -> Result<()> {
        fn sources<'a>(
            name: &'a str,
            edges: &'a Dependencies,
            flow: &GeometryFlow,
            memo: &mut BTreeMap<&'a str, u8>,
        ) -> u8 {
            if let Some(&cached) = memo.get(name) {
                return cached;
            }
            let mask = if let Some(source) = name.strip_prefix("@geometry:") {
                let owner = source
                    .rsplit_once('.')
                    .map(|(owner, _)| owner)
                    .unwrap_or_default();
                if flow.overlays.get(owner).copied().unwrap_or(false) {
                    2
                } else {
                    1
                }
            } else if let Some((_, dependencies)) = edges.get(name) {
                dependencies.iter().fold(0, |mask, dependency| {
                    mask | sources(dependency, edges, flow, memo)
                })
            } else {
                0
            };
            memo.insert(name, mask);
            mask
        }

        let mut memo = BTreeMap::new();
        for (owner, value, span) in &self.sinks {
            let overlay = self.overlays.get(owner).copied().unwrap_or(false);
            let invalid_sources = if overlay { 2 } else { 3 };
            if sources(value, edges, self, &mut memo) & invalid_sources != 0 {
                return fail(
                    *span,
                    "layout feedback: a layout binding cannot depend on geometry from its own or a later layout phase",
                );
            }
        }
        Ok(())
    }
}

fn fixed_geometry_labels<'a>(members: &'a [LoweredMember<'_>]) -> BTreeSet<&'a str> {
    let mut axes = BTreeMap::<&str, u8>::new();
    let mut owners = vec!["root"];
    for member in members {
        match member {
            LoweredMember::BeginElement { key, kind, .. } => {
                owners.push(key);
                if matches!(*kind, "Text" | "Label" | "StyledText") {
                    axes.insert(key, 0);
                }
            }
            LoweredMember::EndElement => {
                owners.pop();
            }
            LoweredMember::Binding(binding)
                if binding.value.op == "literal" && binding.value.ty == Type::Length =>
            {
                if let Some(flags) = axes.get_mut(owners.last().unwrap()) {
                    match binding.name.as_str() {
                        "width" => *flags |= 1,
                        "height" => *flags |= 2,
                        _ => {}
                    }
                }
            }
            LoweredMember::StateBinding(state)
                if matches!(state.binding.name.as_str(), "width" | "height") =>
            {
                if let Some(flags) = axes.get_mut(owners.last().unwrap()) {
                    *flags |= 4;
                }
            }
            _ => {}
        }
    }
    axes.into_iter()
        .filter_map(|(owner, flags)| (flags == 3).then_some(owner))
        .collect()
}

fn geometry_safe_binding(kind: &str, property: &str, fixed_label: bool) -> bool {
    matches!(
        property,
        "background"
            | "color"
            | "border-color"
            | "link-color"
            | "accessible-name"
            | "accessible-description"
            | "opacity"
            | "z"
    ) || scroll_control(kind) && matches!(property, "position" | "size" | "minimum-size" | "policy")
        || matches!(kind, "ProgressIndicator" | "Spinner") && property == "progress"
        || is_window(kind) && property == "title"
        || property == "text" && fixed_label
}

fn collect_bindings(
    members: &[LoweredMember<'_>],
    definitions: &BTreeMap<&str, &[LoweredMember<'_>]>,
    prefix: &str,
    edges: &mut Dependencies,
    links: &mut BTreeMap<String, String>,
    geometry: &mut GeometryFlow,
    inherited_overlay: bool,
) {
    let instances: BTreeSet<_> = members
        .iter()
        .filter_map(|m| match m {
            LoweredMember::BeginElement {
                key,
                instance: true,
                ..
            } => Some(key.as_str()),
            _ => None,
        })
        .collect();
    let stacks: BTreeSet<_> = members
        .iter()
        .filter_map(|member| match member {
            LoweredMember::BeginElement {
                key,
                kind: "StackView",
                ..
            } => Some(key.as_str()),
            _ => None,
        })
        .collect();
    let path = |owner: &str, name: &str| {
        if instances.contains(owner) {
            format!("{prefix}{owner}/root.{name}")
        } else {
            format!("{prefix}{owner}.{name}")
        }
    };
    let scope_path = |owner: &str| path(owner, "").trim_end_matches('.').to_owned();
    let fixed_labels = fixed_geometry_labels(members);

    fn dependencies(e: &ir::Expr, path: &impl Fn(&str, &str) -> String, out: &mut Vec<String>) {
        // The host publishes these values, so they are inputs to the graph.
        if matches!(
            e.op.as_str(),
            "focus_read"
                | "image_playback_read"
                | "scale_rotate_read"
                | "swipe_read"
                | "hover_read"
                | "touch_area_read"
                | "split_resizing"
                | "flick_motion_read"
        ) || e.op == "viewport_read" && !matches!(e.value.as_str(), "content-x" | "content-y")
        {
            return;
        }
        if let Some(owner) = &e.owner {
            if e.op == "scroll_control_read" && e.value == "position" {
                out.push(path(owner, "size"));
            }
            let name = if e.op == "tree_query" {
                "@tree"
            } else if e.op == "view_index" {
                "@view-rows"
            } else if e.op == "stack_read" {
                "@stack"
            } else {
                &e.value
            };
            let target = path(owner, name);
            out.push(if e.op == "geometry_read" {
                format!("@geometry:{target}")
            } else if e.op == "call" {
                format!("{target}()")
            } else {
                target
            });
        }
        for argument in &e.args {
            dependencies(argument, path, out);
        }
    }

    let mut owners = vec!["root"];
    let mut layout_scopes = vec![("", inherited_overlay)];
    let mut alias_handlers = Vec::new();
    for m in members {
        let (name, value, span): (&str, &ir::Expr, Span) = match m {
            LoweredMember::BeginElement {
                key,
                kind,
                instance,
                span,
                ..
            } => {
                if table_view(kind) {
                    let mut inputs = vec![path(key, "undoable"), path(key, "pasteable")];
                    if *kind == "StandardTableView" {
                        inputs.push(path(key, "model"));
                    }
                    for name in ["can-undo", "can-redo"] {
                        edges.insert(path(key, name), (*span, inputs.clone()));
                    }
                } else if *kind == "TableRow" {
                    let table = owners.last().unwrap();
                    for name in ["can-undo", "can-redo"] {
                        if let Some((_, inputs)) = edges.get_mut(&path(table, name)) {
                            inputs.push(path(key, "@model"));
                        }
                    }
                }
                if *kind == "TreeRow"
                    && layout_scopes
                        .last()
                        .is_some_and(|(kind, _)| *kind == "TreeView")
                {
                    let tree = owners.last().unwrap();
                    edges.insert(
                        path(tree, "@tree"),
                        (
                            *span,
                            vec![
                                path(key, "@model"),
                                path(tree, "root-key"),
                                path(tree, "filter-text"),
                            ],
                        ),
                    );
                }
                if !*instance && viewport_kind(kind) && !content_viewport(kind) {
                    let inputs = match *kind {
                        "TreeView" => vec![path(key, "@tree")],
                        "StandardTableView" => vec![
                            path(key, "model"),
                            path(key, "filter-text"),
                            path(key, "auto-sort"),
                        ],
                        "StandardListView" => vec![path(key, "model")],
                        _ => Vec::new(),
                    };
                    edges.insert(path(key, "@view-rows"), (*span, inputs));
                }
                owners.push(key);
                let overlay =
                    layout_scopes.last().unwrap().1 || matches!(*kind, "PopupWindow" | "Tooltip");
                layout_scopes.push((kind, overlay));
                geometry.overlays.insert(scope_path(key), overlay);
                if *instance {
                    collect_bindings(
                        definitions[kind],
                        definitions,
                        &format!("{prefix}{key}/"),
                        edges,
                        links,
                        geometry,
                        overlay,
                    );
                } else if viewport_kind(kind) {
                    for offset in ["content-x", "content-y"] {
                        edges.insert(path(key, offset), (*span, Vec::new()));
                    }
                }
                continue;
            }
            LoweredMember::ScrollTarget { target, span } => {
                let owner = owners.last().unwrap();
                let source = format!("@geometry:{}", path(target, "scroll-range"));
                for metric in ["position", "size"] {
                    edges.insert(
                        path(owner, metric),
                        (*span, vec![source.clone(), path(owner, "orientation")]),
                    );
                }
                continue;
            }
            LoweredMember::EndElement => {
                owners.pop();
                layout_scopes.pop();
                continue;
            }
            LoweredMember::CheckGroupRef {
                group,
                property,
                span,
            } => {
                let member = owners.last().unwrap();
                edges
                    .entry(path(group, "check-state"))
                    .or_insert((*span, Vec::new()))
                    .1
                    .push(path(member, property));
                continue;
            }
            LoweredMember::Condition(value) | LoweredMember::Repeat { model: value, .. } => {
                let owner = owners.last().unwrap();
                let key = path(
                    owner,
                    if matches!(m, LoweredMember::Condition(_)) {
                        "@visible"
                    } else {
                        "@model"
                    },
                );
                let mut deps = Vec::new();
                dependencies(value, &path, &mut deps);
                edges.insert(key.clone(), (value.span, deps));
                if matches!(m, LoweredMember::Repeat { .. })
                    && let [.., parent, _] = owners.as_slice()
                    && let Some((_, inputs)) = edges.get_mut(&path(parent, "@view-rows"))
                {
                    inputs.push(key.clone());
                }
                geometry.sinks.push((scope_path(owner), key, value.span));
                continue;
            }
            LoweredMember::StateSelection(selection) => {
                let mut deps = Vec::new();
                for (_, condition) in &selection.cases {
                    dependencies(condition, &path, &mut deps);
                }
                edges.insert(path(&selection.owner, "@state"), (selection.span, deps));
                continue;
            }
            LoweredMember::StateBinding(state) => {
                let owner = owners.last().unwrap();
                let binding = &state.binding;
                if state.instance_input {
                    let destination = instance_storage(
                        path(owner, &binding.name),
                        &format!("{prefix}{owner}/"),
                        links,
                    );
                    let mut deps = vec![path(&state.owner, "@state")];
                    dependencies(&binding.value, &path, &mut deps);
                    edges
                        .entry(format!("@state-input:{destination}"))
                        .or_insert((binding.span, Vec::new()))
                        .1
                        .extend(deps);
                } else if !geometry_safe_binding(
                    layout_scopes.last().unwrap().0,
                    &binding.name,
                    fixed_labels.contains(owner),
                ) {
                    let key = path(owner, &format!("@state.{}.{}", state.case, binding.name));
                    let mut deps = vec![path(&state.owner, "@state")];
                    dependencies(&binding.value, &path, &mut deps);
                    edges.insert(key.clone(), (binding.span, deps));
                    geometry.sinks.push((scope_path(owner), key, binding.span));
                }
                continue;
            }
            LoweredMember::InstanceLayout(binding) => {
                let owner = owners.last().unwrap();
                if !geometry_safe_binding(layout_scopes.last().unwrap().0, &binding.name, false) {
                    let key = path(owner, &format!("@layout.{}", binding.name));
                    let mut deps = Vec::new();
                    dependencies(&binding.value, &path, &mut deps);
                    edges.insert(key.clone(), (binding.span, deps));
                    geometry.sinks.push((scope_path(owner), key, binding.span));
                }
                continue;
            }
            LoweredMember::Animation(animation) if animation.enabled.is_some() => {
                let owner = owners.last().unwrap();
                if !geometry_safe_binding(
                    layout_scopes.last().unwrap().0,
                    &animation.property,
                    false,
                ) {
                    let key = path(owner, &format!("@animate.{}", animation.property));
                    let mut deps = Vec::new();
                    dependencies(animation.enabled.as_ref().unwrap(), &path, &mut deps);
                    edges.insert(key.clone(), (animation.span, deps));
                    geometry
                        .sinks
                        .push((scope_path(owner), key, animation.span));
                }
                continue;
            }
            LoweredMember::StateTiming(timing) => {
                let owner = owners.last().unwrap();
                let animation = &timing.animation;
                if !geometry_safe_binding(
                    layout_scopes.last().unwrap().0,
                    &animation.property,
                    false,
                ) {
                    let key = path(
                        owner,
                        &format!(
                            "@transition.{}.{}.{}",
                            timing.case, timing.direction, animation.property
                        ),
                    );
                    let mut deps = vec![path(&timing.owner, "@state")];
                    if let Some(enabled) = &animation.enabled {
                        dependencies(enabled, &path, &mut deps);
                    }
                    edges.insert(key.clone(), (animation.span, deps));
                    geometry
                        .sinks
                        .push((scope_path(owner), key, animation.span));
                }
                continue;
            }
            LoweredMember::Property(p) => (&p.name, &p.init, p.span),
            LoweredMember::Callback(callback) | LoweredMember::InstanceHandler(callback) => {
                let mut deps = Vec::new();
                for statement in &callback.body {
                    match statement {
                        ir::Statement::Assignment(assignment) => {
                            dependencies(&assignment.value, &path, &mut deps)
                        }
                        ir::Statement::Invoke(call) => {
                            deps.push(format!("{}()", path(&call.owner, &call.name)));
                            for arg in &call.args {
                                dependencies(arg, &path, &mut deps);
                            }
                        }
                        ir::Statement::Log { level, message, .. } => {
                            dependencies(level, &path, &mut deps);
                            dependencies(message, &path, &mut deps);
                        }
                        ir::Statement::Navigate(navigation) => {
                            if let Some(page) = &navigation.page {
                                dependencies(page, &path, &mut deps);
                            }
                        }
                        ir::Statement::ScrollCommand(command) => {
                            for arg in &command.args {
                                dependencies(arg, &path, &mut deps);
                            }
                        }
                        ir::Statement::SourceSelection(command) => {
                            for arg in &command.args {
                                dependencies(arg, &path, &mut deps);
                            }
                        }
                        ir::Statement::TextCommand(command) => {
                            for arg in &command.args {
                                dependencies(arg, &path, &mut deps);
                            }
                        }
                        ir::Statement::MoveColumn(command) | ir::Statement::MoveRow(command) => {
                            for position in &command.positions {
                                dependencies(position, &path, &mut deps);
                            }
                        }
                        ir::Statement::SetRowHeight(command) => {
                            for argument in &command.arguments {
                                dependencies(argument, &path, &mut deps);
                            }
                        }
                        ir::Statement::TableSelectionCommand(command) => {
                            for argument in &command.args {
                                dependencies(argument, &path, &mut deps);
                            }
                        }
                        ir::Statement::TableSortCommand(command) => {
                            for argument in &command.args {
                                dependencies(argument, &path, &mut deps);
                            }
                        }
                        ir::Statement::RestoreSplit(command) => {
                            dependencies(&command.state, &path, &mut deps);
                        }
                        ir::Statement::TreeCommand(command) => {
                            for argument in &command.args {
                                dependencies(argument, &path, &mut deps);
                            }
                        }
                        ir::Statement::WindowCommand(command) => {
                            if let Some(argument) = &command.argument {
                                dependencies(argument, &path, &mut deps);
                            }
                        }
                        ir::Statement::Media(command) => {
                            if let Some(position) = &command.position {
                                dependencies(position, &path, &mut deps);
                            }
                        }
                        ir::Statement::CheckGroupCommand(command) => {
                            dependencies(&command.state, &path, &mut deps)
                        }
                        ir::Statement::Timer(_)
                        | ir::Statement::Popup(_)
                        | ir::Statement::NativeDialogOpen(_)
                        | ir::Statement::SecondaryWindowCommand(_)
                        | ir::Statement::FocusCommand(_)
                        | ir::Statement::TableHistoryCommand { .. }
                        | ir::Statement::SwipeCancel { .. }
                        | ir::Statement::CancelFlick { .. }
                        | ir::Statement::ReturnToBounds { .. }
                        | ir::Statement::WindowClose { .. } => {}
                    }
                }
                if let Some(result) = &callback.result {
                    dependencies(result, &path, &mut deps);
                }
                let owner = owners.last().unwrap();
                let slot = format!("{}()", path(owner, &callback.name));
                if let Some(target) = &callback.alias {
                    let target = format!("{}()", path(&target.owner, &target.name));
                    if callback.has_handler {
                        alias_handlers.push((target.clone(), callback.span, deps));
                    }
                    links.insert(slot.clone(), target.clone());
                    edges.insert(slot, (callback.span, vec![target]));
                } else if matches!(m, LoweredMember::InstanceHandler(_)) {
                    let destination = terminal_binding(slot, links);
                    edges
                        .entry(destination)
                        .or_insert((callback.span, Vec::new()))
                        .1
                        .extend(deps);
                } else {
                    edges.insert(slot, (callback.span, deps));
                }
                continue;
            }
            LoweredMember::Binding(b) | LoweredMember::InstanceLink(b)
                if instances.contains(owners.last().unwrap()) =>
            {
                (&b.name, &b.value, b.span)
            }
            LoweredMember::Binding(b)
                if b.name == "initial-page" && stacks.contains(owners.last().unwrap()) =>
            {
                ("@stack", &b.value, b.span)
            }
            LoweredMember::Binding(b) | LoweredMember::ControlLink(b) => {
                (&b.name, &b.value, b.span)
            }
            _ => continue,
        };
        let mut deps = Vec::new();
        dependencies(value, &path, &mut deps);
        let owner = owners.last().unwrap();
        let mut destination = path(owner, name);
        if !instances.contains(owner)
            && matches!(m, LoweredMember::Binding(_) | LoweredMember::ControlLink(_))
            && !geometry_safe_binding(
                layout_scopes.last().unwrap().0,
                name,
                fixed_labels.contains(owner),
            )
        {
            geometry
                .sinks
                .push((scope_path(owner), destination.clone(), span));
        }
        if value.op == "control_read"
            && let Some(target) = &value.owner
        {
            geometry
                .sinks
                .push((scope_path(target), path(target, &value.value), span));
        }
        if instances.contains(owner) {
            // Keep alias edges in the instance. The caller replaces the
            // initializer at the storage endpoint reached by those edges.
            destination = instance_storage(destination, &format!("{prefix}{owner}/"), links);
        }
        let forwards = matches!(m, LoweredMember::Property(p) if p.alias)
            || matches!(
                m,
                LoweredMember::ControlLink(_) | LoweredMember::InstanceLink(_)
            );
        if forwards
            && value.op != "check_group_read"
            && let [target] = deps.as_slice()
        {
            links.insert(destination.clone(), target.clone());
        }
        edges.insert(destination, (span, deps));
    }
    for (target, span, dependencies) in alias_handlers {
        edges
            .entry(terminal_binding(target, links))
            .or_insert((span, Vec::new()))
            .1
            .extend(dependencies);
    }
}

fn terminal_binding(mut name: String, links: &BTreeMap<String, String>) -> String {
    let mut seen = BTreeSet::new();
    while let Some(next) = links.get(&name)
        && seen.insert(name.clone())
    {
        name = next.clone();
    }
    name
}

fn instance_storage(mut name: String, scope: &str, links: &BTreeMap<String, String>) -> String {
    let mut seen = BTreeSet::new();
    while let Some(next) = links.get(&name)
        && next.starts_with(scope)
        && seen.insert(name.clone())
    {
        name = next.clone();
    }
    name
}
