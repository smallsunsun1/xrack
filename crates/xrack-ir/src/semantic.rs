use crate::model as ir;
use std::collections::{BTreeMap, BTreeSet};
use xrack_syntax::ast::{self as ast, Diagnostic, ExprKind, Member, Span, Type};

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

fn check_name(name: &str, span: Span) -> Result<()> {
    let n = rust_name(name);
    if ["_", "self", "Self", "super", "crate", "root"].contains(&n.as_str()) || n.starts_with("__")
    {
        return fail(span, format!("reserved identifier `{name}`"));
    }
    Ok(())
}

fn stack_read_type(name: &str) -> Option<Type> {
    Some(match name {
        "current-page" => Type::String,
        "depth" => Type::Int,
        "can-go-back" => Type::Bool,
        _ => return None,
    })
}

pub fn builtin_property(kind: &str, name: &str) -> Option<Type> {
    if !is_builtin(kind) {
        return None;
    }
    if kind == "SecondaryWindow" {
        return (name == "visible").then_some(Type::Bool);
    }
    if kind == "Action" {
        return match name {
            "text" | "shortcut" => Some(Type::String),
            "icon" => Some(Type::Image),
            "enabled" | "checkable" | "checked" => Some(Type::Bool),
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
    if kind == "Label" {
        return builtin_property("Text", name);
    }
    if kind == "StackView"
        && let Some(ty) = stack_read_type(name)
    {
        return Some(ty);
    }
    Some(match name {
        "accessible-name" | "accessible-description" => Type::String,
        "color-scheme" if kind == "Window" => Type::ColorScheme,
        "accent-color" if kind == "Window" => Type::Color,
        "enabled" if matches!(kind, "DragArea" | "DropArea") => Type::Bool,
        "data" | "drag-key" if kind == "DragArea" => Type::String,
        "allow-copy" | "allow-move" | "allow-link" if kind == "DragArea" => Type::Bool,
        "keys" if kind == "DropArea" => Type::StringList,
        "accept-files" if kind == "DropArea" => Type::Bool,
        "file-extensions" if kind == "DropArea" => Type::StringList,
        "drop-action" if kind == "DropArea" => Type::DragAction,
        "default-font-family" if kind == "Window" => Type::String,
        "default-font-size" if kind == "Window" => Type::Length,
        "default-font-weight" if kind == "Window" => Type::Int,
        "default-font-italic" if kind == "Window" => Type::Bool,
        "background"
            if matches!(
                kind,
                "Window"
                    | "Rectangle"
                    | "Page"
                    | "PageHeader"
                    | "PageFooter"
                    | "SwipeContent"
                    | "SwipeLeft"
                    | "SwipeRight"
                    | "Button"
                    | "ToolButton"
                    | "DelayButton"
                    | "DialogButton"
                    | "GroupBox"
                    | "ToolBar"
                    | "TabBar"
                    | "Tumbler"
                    | "Calendar"
                    | "TimePicker"
                    | "TimePickerPopup"
                    | "DatePickerPopup"
                    | "ColorPickerPopup"
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
        "font-size" if has_font(kind) => Type::Length,
        "font-family" if has_font(kind) => Type::String,
        "font-weight" if has_font(kind) => Type::Int,
        "font-italic" if has_font(kind) => Type::Bool,
        "enabled"
            if matches!(
                kind,
                "Button"
                    | "ToolButton"
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
                    | "RadioGroup"
                    | "TabWidget"
                    | "TabBar"
                    | "SwipeView"
                    | "PageIndicator"
                    | "SplitView"
                    | "FlowLayout"
                    | "ToolBar"
            ) =>
        {
            Type::Orientation
        }
        "handle-size" if kind == "SplitView" => Type::Length,
        "handle-color" if kind == "SplitView" => Type::Color,
        "delay" if matches!(kind, "Tooltip" | "DelayButton") => Type::Int,
        "timeout" if kind == "Tooltip" => Type::Int,
        "progress-color" if kind == "DelayButton" => Type::Color,
        "value" | "minimum" | "maximum" | "step-size" if kind == "SpinBox" => Type::Int,
        "editable" if matches!(kind, "SpinBox" | "DoubleSpinBox" | "ComboBox") => Type::Bool,
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
        "value-role" if kind == "ComboBox" => Type::String,
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
        "live" if kind == "SearchField" => Type::Bool,
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
            if matches!(kind, "Calendar" | "DatePickerPopup") =>
        {
            Type::Date
        }
        "display-month" if kind == "Calendar" => Type::Date,
        "week-start" if matches!(kind, "Calendar" | "DatePickerPopup") => Type::Int,
        "weekday-labels" if matches!(kind, "Calendar" | "DatePickerPopup") => Type::StringList,
        "time" if time_picker(kind) => Type::Time,
        "use-24-hour-format" | "show-seconds" if time_picker(kind) => Type::Bool,
        "accept-text" | "cancel-text" if picker_popup(kind) => Type::String,
        "show-alpha" if kind == "ColorPickerPopup" => Type::Bool,
        "interactive" if matches!(kind, "SwipeView" | "PageIndicator") => Type::Bool,
        "open-side" if kind == "SwipeDelegate" => Type::SwipeSide,
        "initial-page" if kind == "StackView" => Type::String,
        "transition-duration" if kind == "StackView" => Type::Int,
        "dot-size" | "spacing" if kind == "PageIndicator" => Type::Length,
        "current-row" if table_view(kind) => Type::Int,
        "current-key" if kind == "TreeView" => Type::Int,
        "key" | "parent-key" if kind == "TreeRow" => Type::Int,
        "indentation" if kind == "TreeView" => Type::Length,
        "row-height"
            if matches!(kind, "ListView" | "StandardListView" | "TreeView") || table_view(kind) =>
        {
            Type::Length
        }
        "header-height" if table_view(kind) => Type::Length,
        "resizable-columns" if table_view(kind) => Type::Bool,
        "multi-select" if table_view(kind) || kind == "TreeView" => Type::Bool,
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
        "icon" if matches!(kind, "Button" | "ToolButton" | "DelayButton" | "MenuItem") => {
            Type::Image
        }
        "icon-size" if matches!(kind, "Button" | "ToolButton" | "DelayButton") => Type::Length,
        "icon-only" if matches!(kind, "Button" | "ToolButton" | "DelayButton") => Type::Bool,
        "wrap" if matches!(kind, "ToolBar" | "DialogButtonBox") => Type::Bool,
        "auto-complete" if kind == "ComboBox" => Type::Bool,
        "placeholder-text" if text_input(kind) || kind == "ComboBox" => Type::String,
        "read-only"
            if text_input(kind)
                || matches!(kind, "SpinBox" | "DoubleSpinBox" | "TableCell" | "TreeCell") =>
        {
            Type::Bool
        }
        "padding" if matches!(kind, "TableCell" | "TreeCell" | "Tooltip") => Type::Length,
        "content-width" | "content-height" | "viewport-height" if kind == "ScrollView" => {
            Type::Length
        }
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
                "Button" | "ToolButton" | "DialogButton" | "DelayButton"
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
                "Rectangle" | "Button" | "ToolButton" | "DialogButton" | "Dialog" | "Drawer"
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
                "Rectangle" | "Button" | "ToolButton" | "DialogButton" | "Dialog" | "Drawer"
            ) || text_input(kind) =>
        {
            Type::Length
        }
        "border-color"
            if matches!(
                kind,
                "Rectangle" | "Button" | "ToolButton" | "DialogButton" | "Dialog" | "Drawer"
            ) || text_input(kind) =>
        {
            Type::Brush
        }
        "source" if kind == "Image" => Type::Image,
        "image-fit" if kind == "Image" => Type::ImageFit,
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

pub fn layout_item_property(name: &str) -> Option<Type> {
    Some(match name {
        "width" | "height" | "x" | "y" | "min-width" | "min-height" | "preferred-width"
        | "preferred-height" | "max-width" | "max-height" => Type::Length,
        "horizontal-stretch" | "vertical-stretch" => Type::Float,
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

pub fn is_builtin(s: &str) -> bool {
    linear_layout(s)
        || text_input(s)
        || matches!(
            s,
            "Window"
                | "Rectangle"
                | "Page"
                | "Text"
                | "Label"
                | "Button"
                | "StandardButton"
                | "Action"
                | "Timer"
                | "FileDialog"
                | "MessageDialog"
                | "SecondaryWindow"
                | "Shortcut"
                | "ToolButton"
                | "DelayButton"
                | "DialogButton"
                | "Dialog"
                | "Drawer"
                | "MenuBar"
                | "Menu"
                | "MenuItem"
                | "MenuSeparator"
                | "ContextMenuArea"
                | "TouchArea"
                | "DragArea"
                | "DropArea"
                | "Image"
                | "Path"
                | "GridLayout"
                | "Row"
                | "ScrollView"
                | "ListView"
                | "GridView"
                | "StandardListView"
                | "TableView"
                | "StandardTableView"
                | "TableColumn"
                | "TableRow"
                | "TableCell"
                | "TreeCell"
                | "SplitView"
                | "Tooltip"
                | "TreeView"
                | "TreeRow"
                | "CheckBox"
                | "Switch"
                | "Slider"
                | "Dial"
                | "RangeSlider"
                | "ProgressIndicator"
                | "Spinner"
                | "BusyIndicator"
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
                | "TimePickerPopup"
                | "DatePickerPopup"
                | "ColorPickerPopup"
                | "Tumbler"
                | "SwipePage"
                | "StackView"
                | "StackPage"
        )
}

fn text_input(kind: &str) -> bool {
    matches!(
        kind,
        "TextInput" | "LineEdit" | "SearchField" | "TextEdit" | "TextArea"
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
        "DatePickerPopup" | "TimePickerPopup" | "ColorPickerPopup"
    )
}

fn time_picker(kind: &str) -> bool {
    matches!(kind, "TimePicker" | "TimePickerPopup")
}

fn table_view(kind: &str) -> bool {
    matches!(kind, "TableView" | "StandardTableView")
}

fn control_event(kind: &str, event: &str) -> bool {
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
        || matches!(kind, "SpinBox" | "DoubleSpinBox") && event == "edited"
        || matches!(kind, "ComboBox" | "RadioGroup") && event == "selected"
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
                "current-row-changed" | "selection-changed" | "sort-ascending" | "sort-descending"
            )
        || kind == "Dialog"
            && matches!(
                event,
                "accepted" | "rejected" | "applied" | "reset" | "help-requested" | "discarded"
            )
        || matches!(kind, "Drawer" | "PopupWindow") && matches!(event, "opened" | "closed")
        || kind == "StackView" && event == "changed"
        || kind == "Calendar" && matches!(event, "changed" | "month-changed")
        || kind == "TimePicker" && event == "changed"
        || kind == "SwipeDelegate" && matches!(event, "clicked" | "changed")
        || kind == "Timer" && event == "triggered"
        || kind == "FileDialog" && matches!(event, "accepted" | "canceled" | "failed")
        || kind == "MessageDialog"
            && matches!(event, "accepted" | "rejected" | "finished" | "failed")
        || kind == "Window" && event == "close-requested"
        || picker_popup(kind) && matches!(event, "accepted" | "canceled")
        || matches!(kind, "MenuItem" | "Action" | "DelayButton") && event == "activated"
        || kind == "Shortcut" && matches!(event, "activated" | "activated-ambiguously")
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
    (matches!(
        kind,
        "Button" | "StandardButton" | "ToolButton" | "TouchArea"
    ) && event == "clicked")
        || control_event(kind, event)
}

/// Validated members of one source component, in lowering order.
pub enum LoweredMember<'a> {
    Property(ir::Property),
    Callback(ir::Callback),
    BeginElement {
        kind: &'a str,
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
    InstanceLayout(ir::Binding),
    Condition(ir::Expr),
    ControlLink(ir::Binding),
    RowLink(ir::Binding),
    Animation(ir::Binding),
    Repeat {
        model: ir::Expr,
        item: String,
        index: Option<String>,
    },
    Handler(ir::Callback),
    EndElement,
}

/// Hand off each component only after its names, types and bindings are valid.
pub fn for_each_component(
    doc: &ast::Document,
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
    let mut inherited = BTreeMap::new();
    let mut active = BTreeSet::new();
    let components = doc
        .components
        .iter()
        .map(|c| inherit(c, &definitions, &mut inherited, &mut active))
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

    let mut validated = Vec::new();
    for c in &components {
        match component(c, &definitions, &records) {
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
                Type::Record(_)
                    | Type::RecordList(_)
                    | Type::StringList
                    | Type::GradientStop
                    | Type::Percent
            ) {
                return fail(field.span, "record fields require scalar or enum types");
            }
        }
        if fields.is_empty() {
            return fail(record.span, "record must declare at least one field");
        }
        records.insert(record.name.as_str(), record);
    }
    Ok(records)
}

fn inherit<'a>(
    component: &'a ast::Component,
    definitions: &BTreeMap<&str, &'a ast::Component>,
    expanded: &mut BTreeMap<&'a str, ast::Component>,
    active: &mut BTreeSet<&'a str>,
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
            span: component.span,
            message: format!("unknown base element or component `{}`", component.base),
        })?;
    let base = inherit(base, definitions, expanded, active)?;
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
                    if binding.two_way || !matches!(property.visibility.as_str(), "in" | "in-out") {
                        return fail(
                            binding.span,
                            format!("cannot override inherited property `{}`", binding.name),
                        );
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
    kind: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowTemplate {
    Table,
    Tree,
}

#[derive(Clone, Default)]
struct Scope<'a> {
    owners: BTreeMap<String, Owner<'a>>,
    parameters: BTreeMap<String, Type>,
    effects_allowed: bool,
    writable_row: Option<String>,
    row_template: Option<RowTemplate>,
    records: BTreeMap<&'a str, &'a ast::RecordDef>,
}

fn key(e: &ast::Element, parent: &str, index: usize) -> String {
    match &e.id {
        Some(id) => id.clone(),
        None => format!("__{parent}_{index}"),
    }
}

fn contains_element<'a>(
    kind: &'a str,
    members: &'a [Member],
    definitions: &BTreeMap<&str, &'a ast::Component>,
    visited: &mut BTreeSet<&'a str>,
    matches_kind: fn(&str) -> bool,
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
    Ok(())
}

fn swipe_slot(kind: &str) -> bool {
    matches!(kind, "SwipeContent" | "SwipeLeft" | "SwipeRight")
}

fn nonvisual_element(kind: &str) -> bool {
    matches!(kind, "Action" | "Timer" | "SecondaryWindow") || native_dialog(kind)
}

fn native_dialog(kind: &str) -> bool {
    matches!(kind, "FileDialog" | "MessageDialog")
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
    ) -> Result<()> {
        let instance = !is_builtin(kind);
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
        let callbacks = if instance || parent.is_none() {
            property_members
                .iter()
                .filter_map(|member| match member {
                    Member::Callback(callback) => Some((callback.name.as_str(), callback)),
                    _ => None,
                })
                .collect()
        } else {
            BTreeMap::new()
        };
        let mut names = BTreeSet::new();
        for p in property_members.iter().filter_map(as_property) {
            check_name(&p.name, p.span)?;
            self.checked_type(&p.ty, p.span)?;
            if !names.insert(rust_name(&p.name)) || builtin_property(kind, &p.name).is_some() {
                return fail(
                    p.span,
                    format!("duplicate or conflicting member `{}`", p.name),
                );
            }
            properties.insert(p.name.as_str(), p);
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
                    kind: kind.into(),
                },
            )
            .is_some()
        {
            return fail(span, format!("duplicate element id `{owner}`"));
        }
        if !instance {
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
                            .is_some_and(|definition| definition.base == "Window")
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
                        if e.ty == "SecondaryWindow" && (owner != "root" || kind != "Window") {
                            return fail(e.span, "SecondaryWindow requires a root Window parent");
                        }
                        let Some(id) = e.id.as_deref().filter(|_| {
                            owner == "root" && e.repeat.is_none() && e.condition.is_none()
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
                        self.register(definitions, &e.members, &e.ty, id, Some(owner), e.span)?;
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
                    if matches!(e.ty.as_str(), "PageHeader" | "PageFooter") {
                        if kind != "Page" || e.repeat.is_some() {
                            return fail(
                                e.span,
                                format!("{} requires a direct Page parent and cannot repeat", e.ty),
                            );
                        }
                        if !page_slots.insert(&e.ty) {
                            return fail(e.span, format!("Page accepts only one {}", e.ty));
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
                            | "Tumbler"
                            | "Spinner"
                            | "BusyIndicator"
                    ) && e.ty != "Tooltip"
                    {
                        return fail(e.span, format!("{kind} only accepts Tooltip children"));
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
                        || e.ty == "Menu" && !matches!(kind, "MenuBar" | "Menu" | "ContextMenuArea")
                        || matches!(e.ty.as_str(), "MenuItem" | "MenuSeparator") && kind != "Menu"
                    {
                        return fail(e.span, format!("{} is not a valid child of {kind}", e.ty));
                    }
                    if e.repeat.is_some() && !is_builtin(&e.ty) {
                        if !matches!(kind, "ListView" | "GridView") || e.id.is_some() {
                            return fail(
                                e.span,
                                "repeated component instances require an anonymous ListView template or an anonymous GridView template",
                            );
                        }
                        if contains_element(
                            &e.ty,
                            &e.members,
                            definitions,
                            &mut BTreeSet::new(),
                            |kind| {
                                matches!(
                                    kind,
                                    "ListView"
                                        | "GridView"
                                        | "StandardListView"
                                        | "TableView"
                                        | "StandardTableView"
                                        | "TreeView"
                                )
                            },
                        ) {
                            return fail(
                                e.span,
                                format!(
                                    "{kind} delegates cannot contain another ListView or other virtual view"
                                ),
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
                        && (e.ty != required
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
                    if (e.ty == "RadioButton" && kind != "RadioGroup")
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
                    self.register(
                        definitions,
                        &e.members,
                        &e.ty,
                        &key(e, owner, index),
                        Some(owner),
                        e.span,
                    )?;
                }
            }
        }
        Ok(())
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
            let owner = if prefix == "self" { current } else { prefix };
            if !self.owners.contains_key(owner) {
                return fail(span, format!("unknown element id `{owner}`"));
            }
            Ok((owner, name))
        } else {
            let mut owner = current;
            loop {
                let scope = &self.owners[owner];
                if contains(scope, reference) {
                    return Ok((owner, reference));
                }
                owner = scope.parent.as_deref().ok_or_else(|| Diagnostic {
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
        if write && self.stack_read(current, reference, span).is_some() {
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
        if scope.instance && p.visibility == "private" {
            return fail(span, format!("property `{reference}` is private"));
        }
        if write
            && ((scope.instance && p.visibility == "out")
                || (!scope.instance && p.visibility == "in"))
        {
            return fail(
                span,
                format!("property `{reference}` is read-only in this scope"),
            );
        }
        Ok((
            ir::PropertyRef {
                owner: owner.into(),
                name: name.into(),
            },
            p.ty.clone(),
        ))
    }

    fn stack_read(&self, current: &str, reference: &str, span: Span) -> Option<ir::Expr> {
        let (receiver, name) = reference.rsplit_once('.')?;
        let owner = if receiver == "self" {
            current
        } else {
            receiver
        };
        if self.owners.get(owner)?.kind != "StackView" {
            return None;
        }
        Some(ir::Expr {
            op: "stack_read".into(),
            owner: Some(owner.into()),
            value: name.into(),
            ty: stack_read_type(name)?,
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
        let callback = self.owners[owner]
            .callbacks
            .get(name)
            .ok_or_else(|| Diagnostic {
                span,
                message: format!("unknown callback `{reference}`"),
            })?;
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

    fn effect_call(&self, call: &ast::Call, current: &str) -> Result<ir::Statement> {
        if let Some((receiver, method)) = call.callee.rsplit_once('.') {
            let owner = if receiver == "self" {
                current
            } else {
                receiver
            };
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
                && self.owners["root"].kind == "Window"
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

fn property(p: &ast::Property, scope: &Scope<'_>, owner: &str) -> Result<ir::Property> {
    let init = match &p.value {
        Some(e) => expression_as(e, scope, owner, p.ty.clone())?,
        None => default(p.ty.clone(), p.span),
    };
    Ok(ir::Property {
        name: p.name.clone(),
        ty: p.ty.clone(),
        visibility: p.visibility.clone(),
        init,
        span: p.span,
    })
}

fn component<'a>(
    c: &'a ast::Component,
    definitions: &BTreeMap<&str, &'a ast::Component>,
    records: &BTreeMap<&'a str, &'a ast::RecordDef>,
) -> Result<Vec<LoweredMember<'a>>> {
    check_name(&c.name, c.span)?;
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
    if matches!(c.base.as_str(), "PageHeader" | "PageFooter") {
        return fail(c.span, format!("{} requires a direct Page parent", c.base));
    }
    if c.base == "Action" {
        return fail(
            c.span,
            "Action requires a named declaration inside a component",
        );
    }
    if matches!(c.base.as_str(), "Timer" | "SecondaryWindow") || native_dialog(&c.base) {
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
            | "TableColumn"
            | "TableRow"
            | "TableCell"
            | "TreeRow"
            | "TreeCell"
    ) {
        return fail(c.span, "component root cannot be a tab or radio option");
    }
    let mut scope = Scope {
        records: records.clone(),
        ..Scope::default()
    };
    scope.register(definitions, &c.members, &c.base, "root", None, c.span)?;
    let mut symbols: BTreeSet<_> = scope.owners["root"]
        .properties
        .keys()
        .map(|name| rust_name(name))
        .collect();
    let mut callback_defs = BTreeMap::new();
    for m in &c.members {
        if let Member::Callback(cb) = m {
            check_name(&cb.name, cb.span)?;
            for parameter in &cb.parameters {
                scope.checked_type(&parameter.ty, cb.span)?;
            }
            if let Some(ty) = &cb.return_type {
                scope.checked_type(ty, cb.span)?;
            }
            if builtin_property(&c.base, &cb.name).is_some()
                || c.base == "Window" && cb.name == "close-requested"
                || !symbols.insert(rust_name(&cb.name))
            {
                return fail(
                    cb.span,
                    format!("duplicate or conflicting member `{}`", cb.name),
                );
            }
            callback_defs.insert(cb.name.clone(), cb);
        }
    }
    let mut lowered = Vec::new();
    for p in c.members.iter().filter_map(as_property) {
        lowered.push(LoweredMember::Property(property(p, &scope, "root")?));
    }
    let mut handlers = BTreeMap::new();
    for h in c.members.iter().filter_map(|m| {
        if let Member::Handler(h) = m {
            Some(h)
        } else {
            None
        }
    }) {
        if control_event(&c.base, &h.name) {
            continue;
        }
        let def = callback_defs.get(&h.name).ok_or_else(|| Diagnostic {
            span: h.span,
            message: format!("unknown callback `{}`", h.name),
        })?;
        if handlers.insert(h.name.clone(), h).is_some() {
            return fail(h.span, format!("duplicate handler `{}`", h.name));
        }
        if def.parameters.len() != h.parameters.len() {
            return fail(
                h.span,
                format!(
                    "callback `{}` expects {} parameters",
                    h.name,
                    def.parameters.len()
                ),
            );
        }
    }
    for def in callback_defs.values() {
        let mut names = BTreeSet::new();
        for p in def
            .parameters
            .iter()
            .filter(|parameter| !parameter.name.is_empty())
        {
            check_name(&p.name, def.span)?;
            if !names.insert(rust_name(&p.name)) {
                return fail(def.span, format!("duplicate parameter `{}`", p.name));
            }
        }
        let mut cb = ir::Callback {
            name: def.name.clone(),
            parameters: def
                .parameters
                .iter()
                .enumerate()
                .map(|(index, parameter)| {
                    if parameter.name.is_empty() {
                        ast::Parameter {
                            name: format!("__xr_arg_{index}"),
                            ty: parameter.ty.clone(),
                        }
                    } else {
                        parameter.clone()
                    }
                })
                .collect(),
            body: vec![],
            span: def.span,
            result: def.return_type.clone().map(|ty| default(ty, def.span)),
            pure: def.pure,
        };
        if let Some(h) = handlers.get(&def.name) {
            let mut bound = BTreeSet::new();
            for name in &h.parameters {
                check_name(name, h.span)?;
                if !bound.insert(rust_name(name)) {
                    return fail(h.span, format!("duplicate handler parameter `{name}`"));
                }
            }
            scope.parameters = h
                .parameters
                .iter()
                .zip(&def.parameters)
                .map(|(name, p)| (name.clone(), p.ty.clone()))
                .collect();
            cb.parameters = h
                .parameters
                .iter()
                .zip(&def.parameters)
                .map(|(name, p)| ast::Parameter {
                    name: name.clone(),
                    ty: p.ty.clone(),
                })
                .collect();
            scope.effects_allowed = !def.pure;
            cb.body = statements(h, &scope, "root")?;
            cb.result = match (def.return_type.clone(), &h.result) {
                (Some(ty), Some(expr)) => Some(expression_as(expr, &scope, "root", ty)?),
                (Some(ty), None) => {
                    return fail(h.span, format!("callback must return {}", ty.name()));
                }
                (None, Some(expr)) => {
                    cb.body.push(discarded_call(expr, &scope, "root")?);
                    None
                }
                (None, None) => None,
            };
            scope.parameters.clear();
            scope.effects_allowed = false;
        }
        lowered.push(LoweredMember::Callback(cb));
    }
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
    lowered.push(LoweredMember::BeginElement {
        kind,
        id,
        key: owner.into(),
        instance,
        span,
    });
    if let Some(condition) = condition {
        let parent = scope.owners[owner].parent.as_deref().unwrap_or("root");
        lowered.push(LoweredMember::Condition(expression_as(
            condition,
            scope,
            parent,
            Type::Bool,
        )?));
    }
    let repeated_scope;
    let scope = if let Some(repeat) = repeat {
        if !instance {
            validate_template(kind, id, members, span, kind == "TreeRow")?;
        }
        check_name(&repeat.item, span)?;
        if let Some(index) = &repeat.index {
            check_name(index, span)?;
            if rust_name(index) == rust_name(&repeat.item) {
                return fail(span, "repeat item and index must have different names");
            }
        }
        let parent = scope.owners[owner].parent.as_deref().unwrap_or("root");
        let model = expression(&repeat.model, scope, parent)?;
        if kind == "TreeRow" {
            if !matches!(model.ty, Type::RecordList(_)) {
                return fail(model.span, "TreeRow requires a record list model");
            }
            for role in ["key", "parent-key"] {
                if !members.iter().any(|member| matches!(member, Member::Binding(binding)
                    if binding.name == role && !binding.two_way && matches!(&binding.value.kind, ExprKind::Reference(reference)
                        if reference.split_once('.').is_some_and(|(item, field)| item == repeat.item && !field.contains('.')))))
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
            _ => None,
        };
        template.writable_row = match &repeat.model.kind {
            ExprKind::Reference(reference)
                if template.row_template.is_some()
                    && model.op == "read"
                    && model.ty.list_item().is_some()
                    && scope.property(parent, reference, true, model.span).is_ok() =>
            {
                Some(repeat.item.clone())
            }
            _ => None,
        };
        template.parameters.insert(repeat.item.clone(), item_type);
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
    let mut bindings = BTreeSet::new();
    let mut handlers = BTreeSet::new();
    let mut animations = BTreeSet::new();
    let mut range_target = None;
    let mut calendar_target = None;
    let mut combo_text_target = None;
    let text_role = if kind == "StandardListView" {
        standard_list_role(members)?
    } else {
        None
    };
    let combo = if kind == "ComboBox" {
        record_combo(members, scope, owner)?
    } else {
        None
    };
    let has_action = matches!(kind, "Button" | "ToolButton" | "MenuItem" | "Shortcut")
        && members
            .iter()
            .any(|member| matches!(member, Member::Binding(binding) if binding.name == "action"));
    for (index, m) in members.iter().enumerate() {
        if kind == "Action" && !matches!(m, Member::Binding(_) | Member::Handler(_)) {
            return fail(
                m.span(),
                "Action only accepts property bindings and activated()",
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
        if instance && !matches!(m, Member::Binding(_)) {
            return fail(
                m.span(),
                "a component instance body only supports input and layout bindings",
            );
        }
        match m {
            Member::Binding(b) => {
                if kind == "StackPage" && b.name == "visible" {
                    return fail(b.span, "StackView manages StackPage visibility");
                }
                if kind == "StackView" && stack_read_type(&b.name).is_some() {
                    return fail(
                        b.span,
                        format!("property `StackView.{}` is read-only", b.name),
                    );
                }
                if !bindings.insert(b.name.as_str()) {
                    return fail(b.span, format!("duplicate binding `{}`", b.name));
                }
                if b.name == "action"
                    && matches!(kind, "Button" | "ToolButton" | "MenuItem" | "Shortcut")
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
                if kind == "Shortcut"
                    && bindings.contains("sequence")
                    && bindings.contains("sequences")
                {
                    return fail(b.span, "Shortcut requires sequence or sequences");
                }
                if has_action && matches!(b.name.as_str(), "checked" | "checkable" | "shortcut") {
                    return fail(
                        b.span,
                        "checked, checkable and shortcut belong to the linked Action",
                    );
                }
                if matches!(kind, "ComboBox" | "RadioGroup")
                    && bindings.contains("current-index")
                    && bindings.contains("current-value")
                {
                    return fail(
                        b.span,
                        format!("{kind} selection accepts either current-index or current-value"),
                    );
                }
                if kind == "CheckBox"
                    && bindings.contains("checked")
                    && (bindings.contains("check-state") || bindings.contains("tristate"))
                {
                    return fail(b.span, "CheckBox accepts checked or check-state/tristate");
                }
                if kind == "Tumbler" && bindings.contains("model") && bindings.contains("count") {
                    return fail(b.span, "Tumbler accepts either model or count");
                }
                if b.two_way {
                    let field_type = match (kind, b.name.as_str()) {
                        ("TableCell" | "TreeCell", "text") => Some(Type::String),
                        _ if scope.row_template.is_some() => row_editor_type(kind, &b.name),
                        _ => None,
                    };
                    if let Some(field_type) = field_type {
                        if !matches!(&b.value.kind, ExprKind::Reference(reference)
                            if scope.writable_row.as_deref() == reference.split('.').next()
                                && (kind == "TableCell" || reference.contains('.')))
                        {
                            let message = if kind == "TableCell" {
                                "editable TableCell text requires the repeated item of a writable [string] property or a string field of a writable record list"
                            } else if kind == "TreeCell" {
                                "editable TreeCell text requires a string field of a writable record list"
                            } else if scope.row_template == Some(RowTemplate::Tree) {
                                "tree editor binding requires a matching field of a writable record list"
                            } else {
                                "table editor binding requires a matching field of a writable record list"
                            };
                            return fail(b.span, message);
                        }
                        let value = expression(&b.value, scope, owner)?;
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
                        ("Calendar", "date" | "display-month") | ("DatePickerPopup", "date") => {
                            Type::Date
                        }
                        ("TimePicker" | "TimePickerPopup", "time") => Type::Time,
                        ("ColorPickerPopup", "color") => Type::Color,
                        ("Slider" | "Dial" | "DoubleSpinBox", "value") => Type::Float,
                        ("RangeSlider", "lower-value" | "upper-value") => Type::Float,
                        ("SpinBox", "value")
                        | (
                            "ComboBox" | "RadioGroup" | "TabWidget" | "TabBar" | "SwipeView"
                            | "ListView" | "GridView" | "StandardListView",
                            "current-index",
                        ) => Type::Int,
                        ("ComboBox", "current-value") => combo
                            .as_ref()
                            .map_or(Type::String, |combo| combo.value_type.clone()),
                        ("RadioGroup", "current-value") => Type::String,
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
                    let (target, ty) = scope.property(owner, reference, true, b.span)?;
                    if kind == "RangeSlider"
                        && range_target.replace(target.clone()).as_ref() == Some(&target)
                    {
                        return fail(
                            b.span,
                            "RangeSlider endpoints require distinct writable properties",
                        );
                    }
                    if kind == "Calendar"
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
                    let record = &scope.records[name.as_ref()];
                    for member in members {
                        let Member::Element(column) = member else {
                            continue;
                        };
                        if column.ty != "TableColumn" {
                            continue;
                        }
                        check_standard_column(column, record)?;
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
                            if !scope.records.get(name.as_ref()).is_some_and(|record| {
                                record.fields.iter().any(|candidate| {
                                    candidate.name == field && candidate.ty == Type::String
                                })
                            }) {
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
                    ty
                } else {
                    builtin_property(kind, &b.name).ok_or_else(|| Diagnostic {
                        span: b.span,
                        message: format!("unknown property `{}.{}`", kind, b.name),
                    })?
                };
                let relative_size = ty == Type::Length
                    && (matches!(
                        b.name.as_str(),
                        "width"
                            | "height"
                            | "min-width"
                            | "min-height"
                            | "max-width"
                            | "max-height"
                            | "preferred-width"
                            | "preferred-height"
                    ) || kind == "ScrollView"
                        && matches!(b.name.as_str(), "content-width" | "content-height"));
                let value = if relative_size {
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
            Member::Handler(h) if !root || control_event(kind, &h.name) => {
                if has_action {
                    return fail(h.span, "activation handler belongs to the linked Action");
                }
                if !builtin_event(kind, &h.name) {
                    return fail(h.span, format!("unknown event `{kind}.{}`", h.name));
                }
                let close_requested = kind == "Window" && h.name == "close-requested";
                if close_requested && !root {
                    return fail(
                        h.span,
                        "close-requested requires the component's root Window",
                    );
                }
                if id.is_none()
                    && !matches!(kind, "MenuItem" | "Shortcut")
                    && !close_requested
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
                                | "TimePicker"
                                | "TimePickerPopup"
                                | "DatePickerPopup"
                                | "ColorPickerPopup"
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
                        ))
                {
                    return fail(
                        h.span,
                        "elements with handlers require an id (`name := Button { ... }`)",
                    );
                }
                let parameter_types: &[Type] = match kind {
                    "Slider" | "Dial" | "DoubleSpinBox" => &[Type::Float],
                    "RangeSlider" => &[Type::Float, Type::Float],
                    "Calendar" => &[Type::Date],
                    "TimePicker" => &[Type::Time],
                    "TimePickerPopup" if h.name == "accepted" => &[Type::Time],
                    "DatePickerPopup" if h.name == "accepted" => &[Type::Date],
                    "ColorPickerPopup" if h.name == "accepted" => &[Type::Color],
                    "FileDialog" if h.name == "accepted" => &[Type::StringList],
                    "MessageDialog" if h.name == "finished" => &[Type::StandardButtonKind],
                    kind if native_dialog(kind) && h.name == "failed" => &[Type::String],
                    "GroupBox" => &[Type::Bool],
                    "CheckBox" if h.name == "check-state-changed" => &[Type::CheckState],
                    "StackView" => &[Type::String, Type::Int],
                    "SwipeDelegate" if h.name == "changed" => &[Type::SwipeSide],
                    "DragArea" if h.name == "drag-finished" => &[Type::DragAction],
                    "DropArea" if matches!(h.name.as_str(), "entered" | "dropped") => {
                        &[Type::String, Type::DragAction]
                    }
                    "DropArea" if h.name == "position-changed" => &[Type::Length, Type::Length],
                    "DropArea" if matches!(h.name.as_str(), "files-entered" | "files-dropped") => {
                        &[Type::StringList]
                    }
                    "SpinBox" | "TabWidget" | "TabBar" | "SwipeView" | "PageIndicator"
                    | "ListView" | "GridView" | "StandardListView" | "TableView"
                    | "StandardTableView" | "TreeView" | "Tumbler" => &[Type::Int],
                    "ComboBox" if h.name == "value-selected" => {
                        let Some(combo) = &combo else {
                            return fail(h.span, "value-selected requires a record ComboBox model");
                        };
                        std::slice::from_ref(&combo.value_type)
                    }
                    "ComboBox" | "RadioGroup" | "SearchField" => &[Type::String],
                    "TextEdit" | "TextArea" => &[Type::String],
                    _ => &[],
                };
                if h.parameters.len() != parameter_types.len() {
                    let message = if parameter_types.is_empty() {
                        format!("event {} takes no parameters", h.name)
                    } else if kind == "RangeSlider" {
                        format!("event {} expects two float parameters", h.name)
                    } else if kind == "StackView" {
                        format!("event {} expects a string page and an int depth", h.name)
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
                let handler_scope = Scope {
                    effects_allowed: true,
                    parameters: parameters
                        .iter()
                        .map(|parameter| (parameter.name.clone(), parameter.ty.clone()))
                        .collect(),
                    ..scope.clone()
                };
                let mut body = statements(h, &handler_scope, owner)?;
                let result = if close_requested {
                    let Some(expr) = &h.result else {
                        return fail(h.span, "close-requested must return a bool");
                    };
                    Some(expression_as(expr, &handler_scope, owner, Type::Bool)?)
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
                }));
            }
            Member::Property(p) if !root => {
                lowered.push(LoweredMember::Property(property(p, scope, owner)?));
            }
            Member::Callback(_) if !root => {
                return fail(m.span(), "declare callbacks at component scope");
            }
            Member::Animation(animation) => {
                let target = &animation.property;
                if !animations.insert(target) {
                    return fail(
                        animation.span,
                        format!("duplicate animation for `{target}`"),
                    );
                }
                if root && kind == "Window" {
                    return fail(
                        animation.span,
                        "animate a visual child instead of the Window",
                    );
                }
                if !matches!(
                    builtin_property(kind, target),
                    Some(Type::Length | Type::Color | Type::Brush | Type::Float | Type::Int)
                ) {
                    return fail(animation.span, format!("cannot animate `{kind}.{target}`"));
                }
                let duration = animation
                    .duration_ms
                    .parse::<u32>()
                    .ok()
                    .filter(|duration| *duration <= 60_000)
                    .ok_or_else(|| Diagnostic {
                        span: animation.span,
                        message: "animation duration must be 0–60000ms".into(),
                    })?;
                lowered.push(LoweredMember::Animation(ir::Binding {
                    name: target.clone(),
                    value: ir::Expr::literal(Type::Int, duration.to_string(), animation.span),
                    span: animation.span,
                }));
            }
            _ => {}
        }
    }
    lowered.push(LoweredMember::EndElement);
    Ok(())
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
    let model = binding("model")
        .map(|model| expression(&model.value, scope, owner))
        .transpose()?;
    let Some(model) = model else {
        if let Some(role) = text_role.or(value_role) {
            return fail(role.span, "ComboBox roles require a record model");
        }
        return Ok(None);
    };
    let Type::RecordList(name) = &model.ty else {
        if let Some(role) = text_role.or(value_role) {
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
    let record = &scope.records[name.as_ref()];
    if !record
        .fields
        .iter()
        .any(|field| field.name == text && field.ty == Type::String)
    {
        return fail(
            model.span,
            format!("ComboBox text-role requires a string field named `{text}`"),
        );
    }
    let Some(field) = record.fields.iter().find(|field| field.name == value) else {
        return fail(
            model.span,
            format!("ComboBox value-role has no record field named `{value}`"),
        );
    };
    if !matches!(
        field.ty,
        Type::String | Type::Int | Type::Float | Type::Bool | Type::Date | Type::Time
    ) {
        return fail(
            model.span,
            "ComboBox value-role requires a string, int, float, bool, Date or Time field",
        );
    }
    Ok(Some(RecordCombo {
        model,
        value_type: field.ty.clone(),
    }))
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

fn check_standard_column(column: &ast::Element, record: &ast::RecordDef) -> Result<()> {
    let Some(field) = column.members.iter().find_map(|member| match member {
        Member::Binding(binding) if binding.name == "field" => Some(binding),
        _ => None,
    }) else {
        return fail(column.span, "StandardTableView column requires a field");
    };
    let ExprKind::Literal {
        value,
        ty: Type::String,
    } = &field.value.kind
    else {
        return fail(
            field.span,
            "TableColumn.field requires a constant string field name",
        );
    };
    let name: String = serde_json::from_str(value).map_err(|error| Diagnostic {
        span: field.span,
        message: error.to_string(),
    })?;
    let Some(definition) = record
        .fields
        .iter()
        .find(|definition| definition.name == name)
    else {
        return fail(
            field.span,
            format!("record `{}` has no field `{name}`", record.name),
        );
    };
    if !matches!(
        definition.ty,
        Type::String | Type::Int | Type::Float | Type::Bool | Type::Date | Type::Time
    ) {
        return fail(
            field.span,
            format!(
                "TableColumn.field `{name}` requires a string, int, float, bool, Date or Time field"
            ),
        );
    }
    Ok(())
}

fn row_editor_binding(kind: &str) -> Option<(&'static str, Type)> {
    match kind {
        "SpinBox" => Some(("value", Type::Int)),
        "Slider" | "Dial" | "DoubleSpinBox" => Some(("value", Type::Float)),
        "CheckBox" | "Switch" => Some(("checked", Type::Bool)),
        _ => None,
    }
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
) -> Result<()> {
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
                | "Button"
                | "StandardButton"
                | "ToolButton"
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
                | "CheckBox"
                | "Switch"
                | "Slider"
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
                | "TimePicker"
                | "TimePickerPopup"
                | "DatePickerPopup"
                | "ColorPickerPopup"
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
            Member::Binding(_) | Member::Animation(_) => {}
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

fn default(ty: Type, span: Span) -> ir::Expr {
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
    if matches!(ty, Type::RecordList(_)) {
        return ir::Expr::operation("record_array", ty, vec![], span);
    }
    if matches!(ty, Type::Record(_)) {
        return ir::Expr::operation("default_record", ty, vec![], span);
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
        Type::ImageFit => "contain",
        Type::LayoutAlignment => "stretch",
        Type::CrossAxisAlignment => "auto",
        Type::Orientation => "horizontal",
        Type::FlexboxLayoutDirection => "row",
        Type::FlexboxLayoutWrap => "wrap",
        Type::DialInputMode => "circular",
        Type::ScrollBarPolicy => "as-needed",
        Type::TextWrap => "word-wrap",
        Type::TextHorizontalAlignment => "start",
        Type::TextVerticalAlignment => "top",
        Type::TextOverflow => "clip",
        Type::InputType => "text",
        Type::ColorScheme => "system",
        Type::FileDialogMode => "open-file",
        Type::MessageLevel => "info",
        Type::MessageButtons => "ok",
        Type::CheckState => "unchecked",
        Type::DialogRole => "accept",
        Type::StandardButtonKind => "ok",
        Type::SwipeSide => "closed",
        Type::DragAction => "none",
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
    if let ExprKind::Array(items) = &e.kind
        && let Type::RecordList(name) = &expected
    {
        let items = items
            .iter()
            .map(|item| expression_as(item, scope, owner, Type::Record(name.clone())))
            .collect::<Result<Vec<_>>>()?;
        return Ok(ir::Expr::operation("record_array", expected, items, e.span));
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
        ExprKind::ImageUrl(url) => {
            let value = serde_json::from_str::<String>(url).map_err(|e| Diagnostic {
                span,
                message: format!("invalid image path: {e}"),
            })?;
            ir::Expr {
                value,
                ..ir::Expr::operation("image_url", Type::Image, vec![], span)
            }
        }
        ExprKind::Array(items) => {
            let items = items
                .iter()
                .map(|item| expression(item, scope, owner))
                .collect::<Result<Vec<_>>>()?;
            let (op, ty, item_ty) = match items.first().map(|item| &item.ty) {
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
            let civil = match name.as_str() {
                "Date" => Some(("make_date", Type::Date, ["year", "month", "day"])),
                "Time" => Some(("make_time", Type::Time, ["hour", "minute", "second"])),
                _ => None,
            };
            let schema = if let Some((_, _, fields)) = &civil {
                fields
                    .iter()
                    .copied()
                    .map(|field| (field, Type::Int))
                    .collect::<Vec<_>>()
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
            let (op, ty) = civil
                .map(|(op, ty, _)| (op, ty))
                .unwrap_or_else(|| ("make_record", Type::Record(name.as_str().into())));
            ir::Expr::operation(op, ty, args, span)
        }
        ExprKind::Call(call) => {
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
            if let Some((namespace, name)) = s.split_once('.')
                && matches!(
                    namespace,
                    "ImageFit"
                        | "LayoutAlignment"
                        | "CrossAxisAlignment"
                        | "Orientation"
                        | "FlexboxLayoutDirection"
                        | "FlexboxLayoutWrap"
                        | "DialInputMode"
                        | "ScrollBarPolicy"
                        | "TextWrap"
                        | "TextHorizontalAlignment"
                        | "TextVerticalAlignment"
                        | "TextOverflow"
                        | "InputType"
                        | "ColorScheme"
                        | "FileDialogMode"
                        | "MessageLevel"
                        | "MessageButtons"
                        | "CheckState"
                        | "DialogRole"
                        | "StandardButtonKind"
                        | "SwipeSide"
                        | "DragAction"
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
                let record = if let Some(ty @ (Type::Record(_) | Type::Date | Type::Time)) =
                    scope.parameters.get(base)
                {
                    Some(ir::Expr {
                        op: "param".into(),
                        value: base.into(),
                        owner: None,
                        ty: ty.clone(),
                        args: vec![],
                        span,
                    })
                } else if let Ok((property, ty @ (Type::Record(_) | Type::Date | Type::Time))) =
                    scope.property(owner, base, false, span)
                {
                    Some(ir::Expr::read(property, ty, span))
                } else {
                    None
                };
                if let Some(record) = record {
                    let civil = match record.ty {
                        Type::Date => Some(("date_field", ["year", "month", "day"])),
                        Type::Time => Some(("time_field", ["hour", "minute", "second"])),
                        _ => None,
                    };
                    if let Some((op, fields)) = civil {
                        if !fields.contains(&field) {
                            let name = if record.ty == Type::Date {
                                "Date"
                            } else {
                                "Time"
                            };
                            return fail(span, format!("{name} has no field `{field}`"));
                        }
                        return Ok(ir::Expr {
                            value: field.into(),
                            ..ir::Expr::operation(op, Type::Int, vec![record], span)
                        });
                    }
                    let Type::Record(name) = &record.ty else {
                        unreachable!()
                    };
                    let ty = scope.records[name.as_ref()]
                        .fields
                        .iter()
                        .find(|definition| definition.name == field)
                        .map(|definition| definition.ty.clone())
                        .ok_or_else(|| Diagnostic {
                            span,
                            message: format!("record `{name}` has no field `{field}`"),
                        })?;
                    return Ok(ir::Expr {
                        value: field.into(),
                        ..ir::Expr::operation("get_field", ty, vec![record], span)
                    });
                }
            }
            if !s.contains('.')
                && let Some(ty) = scope.parameters.get(s)
            {
                return Ok(ir::Expr {
                    op: "param".into(),
                    value: s.clone(),
                    owner: None,
                    ty: ty.clone(),
                    args: vec![],
                    span,
                });
            }
            if let Some(read) = scope.stack_read(owner, s, span) {
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

fn numeric(t: &Type) -> bool {
    matches!(t, Type::Int | Type::Float)
}
fn promote(a: ir::Expr, b: ir::Expr) -> Result<(ir::Expr, ir::Expr)> {
    if matches!(
        (&a.ty, &b.ty),
        (Type::Float, Type::Int) | (Type::Brush, Type::Color)
    ) {
        Ok((a, coerce(b, Type::Float)?))
    } else if matches!(
        (&a.ty, &b.ty),
        (Type::Int, Type::Float) | (Type::Color, Type::Brush)
    ) {
        Ok((coerce(a, Type::Float)?, b))
    } else {
        Ok((a, b))
    }
}

fn binary(op: &str, a: ir::Expr, b: ir::Expr, span: Span) -> Result<ir::Expr> {
    if op == "+" && (a.ty == Type::String || b.ty == Type::String) {
        fn stringify(e: ir::Expr) -> Result<ir::Expr> {
            match e.ty {
                Type::String => Ok(e),
                Type::Int | Type::Float | Type::Bool | Type::Date | Type::Time => Ok(ir::Expr {
                    op: "stringify".into(),
                    value: String::new(),
                    owner: None,
                    ty: Type::String,
                    span: e.span,
                    args: vec![e],
                }),
                _ => fail(
                    e.span,
                    "string concatenation accepts string, int, float, bool, Date or Time",
                ),
            }
        }
        return Ok(ir::Expr {
            op: "binary".into(),
            value: op.into(),
            owner: None,
            ty: Type::String,
            args: vec![stringify(a)?, stringify(b)?],
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
    fn visit<'a>(
        name: &'a str,
        edges: &'a Dependencies,
        active: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
        description: &str,
    ) -> Result<()> {
        if done.contains(name) {
            return Ok(());
        }
        let (span, next) = &edges[name];
        if !active.insert(name) {
            return fail(*span, format!("cyclic {description} involving `{name}`"));
        }
        for dependency in next {
            visit(dependency, edges, active, done, description)?;
        }
        active.remove(name);
        done.insert(name);
        Ok(())
    }

    let mut done = BTreeSet::new();
    let mut active = BTreeSet::new();
    for name in edges.keys() {
        visit(name, edges, &mut active, &mut done, description)?;
    }
    Ok(())
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
    for members in definitions.values() {
        let mut edges = Dependencies::new();
        collect_bindings(members, &definitions, "", &mut edges);
        check_cycles(&edges, "property binding or callback call")?;
    }
    Ok(())
}

fn collect_bindings(
    members: &[LoweredMember<'_>],
    definitions: &BTreeMap<&str, &[LoweredMember<'_>]>,
    prefix: &str,
    edges: &mut Dependencies,
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

    fn dependencies(e: &ir::Expr, path: &impl Fn(&str, &str) -> String, out: &mut Vec<String>) {
        if let Some(owner) = &e.owner {
            let name = if e.op == "stack_read" {
                "@stack"
            } else {
                &e.value
            };
            let target = path(owner, name);
            out.push(if e.op == "call" {
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
    for m in members {
        let (name, value, span): (&str, &ir::Expr, Span) = match m {
            LoweredMember::BeginElement {
                key,
                kind,
                instance,
                ..
            } => {
                owners.push(key);
                if *instance {
                    collect_bindings(
                        definitions[kind],
                        definitions,
                        &format!("{prefix}{key}/"),
                        edges,
                    );
                }
                continue;
            }
            LoweredMember::EndElement => {
                owners.pop();
                continue;
            }
            LoweredMember::Property(p) => (&p.name, &p.init, p.span),
            LoweredMember::Callback(callback) => {
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
                        ir::Statement::Navigate(navigation) => {
                            if let Some(page) = &navigation.page {
                                dependencies(page, &path, &mut deps);
                            }
                        }
                        ir::Statement::Timer(_)
                        | ir::Statement::Popup(_)
                        | ir::Statement::NativeDialogOpen(_)
                        | ir::Statement::SecondaryWindowCommand(_)
                        | ir::Statement::WindowClose { .. } => {}
                    }
                }
                if let Some(result) = &callback.result {
                    dependencies(result, &path, &mut deps);
                }
                edges.insert(
                    format!("{}()", path("root", &callback.name)),
                    (callback.span, deps),
                );
                continue;
            }
            LoweredMember::Binding(b) if instances.contains(owners.last().unwrap()) => {
                (&b.name, &b.value, b.span)
            }
            LoweredMember::Binding(b)
                if b.name == "initial-page" && stacks.contains(owners.last().unwrap()) =>
            {
                ("@stack", &b.value, b.span)
            }
            _ => continue,
        };
        let mut deps = Vec::new();
        dependencies(value, &path, &mut deps);
        // An instance input takes precedence over the component's default binding.
        edges.insert(path(owners.last().unwrap(), name), (span, deps));
    }
}
