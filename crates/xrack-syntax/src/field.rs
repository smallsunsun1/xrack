use crate::ast::Type;

/// Fields of built-in values, including their MLIR operation and Rust member.
#[doc(hidden)]
#[macro_export]
macro_rules! builtin_value_fields {
    ($declare:ident) => {
        $declare! {
            KeyEvent, "key_field" {
                "key" => key: String,
                "text" => text: String,
                "repeat" => repeat: Bool,
                "modifiers" => modifiers: KeyModifiers,
            }
            KeyModifiers, "key_field" {
                "control" => control: Bool,
                "shift" => shift: Bool,
                "alt" => alt: Bool,
                "meta" => meta: Bool,
                "primary" => primary: Bool,
            }
            WheelEvent, "wheel_field" {
                "x" => x: Length,
                "y" => y: Length,
                "delta-x" => delta_x: Length,
                "delta-y" => delta_y: Length,
                "lines-x" => lines_x: Float,
                "lines-y" => lines_y: Float,
                "is-line" => is_line: Bool,
                "modifiers" => modifiers: KeyModifiers,
            }
            PointerEvent, "pointer_field" {
                "kind" => kind: PointerEventKind,
                "button" => button: PointerButton,
                "x" => x: Length,
                "y" => y: Length,
                "inside" => inside: Bool,
                "is-touch" => is_touch: Bool,
                "touch-finger-id" => touch_finger_id: Int,
                "modifiers" => modifiers: KeyModifiers,
            }
            TouchPoint, "touch_point_field" {
                "id" => id: Int,
                "x" => x: Length,
                "y" => y: Length,
                "start-x" => start_x: Length,
                "start-y" => start_y: Length,
                "inside" => inside: Bool,
            }
            Date, "date_field" {
                "year" => year: Int,
                "month" => month: Int,
                "day" => day: Int,
            }
            Time, "time_field" {
                "hour" => hour: Int,
                "minute" => minute: Int,
                "second" => second: Int,
            }
            Font, "font_field" {
                "family" => family: String,
                "size" => size: Length,
                "weight" => weight: Int,
                "italic" => italic: Bool,
            }
        }
    };
}

macro_rules! field_type {
    ($($owner:ident, $op:literal { $($name:literal => $member:ident: $result:ident,)* })*) => {
        impl Type {
            pub fn builtin_field(&self, name: &str) -> Option<(&'static str, Self)> {
                match (self, name) {
                    $($((Self::$owner, $name) => Some(($op, Self::$result)),)*)*
                    _ => None,
                }
            }
        }
    };
}

builtin_value_fields!(field_type);
