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
            Point2D, "point2d_field" {
                "x" => x: Length,
                "y" => y: Length,
            }
            Point3D, "point3d_field" {
                "x" => x: Float,
                "y" => y: Float,
                "z" => z: Float,
            }
            Vector3D, "vector3d_field" {
                "x" => x: Float,
                "y" => y: Float,
                "z" => z: Float,
            }
            Hit3D, "hit3d_field" {
                "valid" => valid: Bool,
                "entering" => entering: Bool,
                "object-id" => object_id: String,
                "row-index" => row_index: Int,
                "mesh-node-index" => mesh_node_index: Int,
                "instance-index" => instance_index: Int,
                "distance" => distance: Float,
                "position" => position: Point3D,
                "normal" => normal: Vector3D,
                "local-position" => local_position: Point3D,
                "local-normal" => local_normal: Vector3D,
                "u" => u: Float,
                "v" => v: Float,
            }
            PhysicsContact3D, "physics_contact3d_field" {
                "body" => body: Int,
                "position" => position: Point3D,
                "normal" => normal: Vector3D,
                "impulse" => impulse: Vector3D,
                "normal-impulse" => normal_impulse: Float,
            }
            PhysicsHit3D, "physics_hit3d_field" {
                "body" => body: Int,
                "distance" => distance: Float,
                "position" => position: Point3D,
                "normal" => normal: Vector3D,
                "sensor" => sensor: Bool,
            }
            WalkResult3D, "walk_result3d_field" {
                "position" => position: Point3D,
                "crossings" => crossings: Hit3DList,
                "contacts" => contacts: Hit3DList,
                "support" => support: Hit3D,
                "grounded" => grounded: Bool,
                "hit-wall" => hit_wall: Bool,
                "hit-ceiling" => hit_ceiling: Bool,
            }
            InstanceTable3D, "instance_table3d_field" {
                "count" => count: Int,
                "loaded" => loaded: Bool,
            }
            RotationTrack3D, "rotation_track3d_field" {
                "valid" => valid: Bool,
                "count" => count: Int,
                "duration" => duration: Float,
            }
            Quaternion3D, "quaternion3d_field" {
                "x" => x: Float,
                "y" => y: Float,
                "z" => z: Float,
                "w" => w: Float,
            }
            Instance3D, "instance3d_field" {
                "valid" => valid: Bool,
                "object-id" => object_id: String,
                "row-index" => row_index: Int,
                "mesh-node-index" => mesh_node_index: Int,
                "instance-index" => instance_index: Int,
                "color" => color: Color,
                "origin" => origin: Point3D,
                "axis-x" => axis_x: Vector3D,
                "axis-y" => axis_y: Vector3D,
                "axis-z" => axis_z: Vector3D,
                "data-x" => data_x: Float,
                "data-y" => data_y: Float,
                "data-z" => data_z: Float,
                "data-w" => data_w: Float,
            }
            Bounds3D, "bounds3d_field" {
                "valid" => valid: Bool,
                "min" => min: Point3D,
                "max" => max: Point3D,
                "center" => center: Point3D,
                "size" => size: Vector3D,
            }
            MeshNode3D, "mesh_node3d_field" {
                "valid" => valid: Bool,
                "source-index" => source_index: Int,
                "parent-index" => parent_index: Int,
                "name" => name: String,
                "path" => path: String,
                "origin" => origin: Point3D,
                "axis-x" => axis_x: Vector3D,
                "axis-y" => axis_y: Vector3D,
                "axis-z" => axis_z: Vector3D,
                "has-mesh" => has_mesh: Bool,
                "has-camera" => has_camera: Bool,
                "has-light" => has_light: Bool,
            }
            MeshAsset3D, "mesh_asset3d_field" {
                "loaded" => is_loaded: Bool,
                "bounds" => bounds: Bounds3D,
            }
            ProjectedPoint3D, "projected_point3d_field" {
                "point" => point: Point2D,
                "depth" => depth: Float,
                "valid" => valid: Bool,
                "inside" => inside: Bool,
            }
            Vector2D, "vector2d_field" {
                "x" => x: Length,
                "y" => y: Length,
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
            pub fn builtin_fields(&self) -> &'static [&'static str] {
                match self {
                    $(Self::$owner => &[$($name),*],)*
                    _ => &[],
                }
            }

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
