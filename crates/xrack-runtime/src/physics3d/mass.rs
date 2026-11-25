//! Geometry-derived and explicit mass properties for dynamic rigid bodies.

use nalgebra::{Matrix3, SymmetricEigen};
use rapier3d::{
    math::Matrix,
    prelude::{ColliderHandle, MassProperties, Pose, RigidBodyHandle, Rotation, Vector},
};

use super::{PhysicsWorld3D, Simulation, contacts::as_vector, scene_pose, vector};
use crate::{Point3D, Vector3D};

enum UniformMass {
    Mass(f32),
    Density(f32),
}

struct MassChange {
    body: RigidBodyHandle,
    collider: ColliderHandle,
    properties: MassProperties,
}

impl PhysicsWorld3D {
    /// Assign total mass and derive the center of mass and inertia from all attached shapes.
    pub fn set_mass(&self, body: i64, mass: f64) -> bool {
        positive(mass).is_some_and(|mass| self.set_uniform_mass(body, UniformMass::Mass(mass)))
    }

    /// Assign a body-specific uniform density in mass per cubic world unit.
    pub fn set_density(&self, body: i64, density: f64) -> bool {
        positive(density)
            .is_some_and(|density| self.set_uniform_mass(body, UniformMass::Density(density)))
    }

    /// Use the world's density now and keep following subsequent changes to it.
    pub fn use_default_density(&self, body: i64) -> bool {
        self.update(|simulation| {
            let change = simulation
                .uniform_mass_properties(body, UniformMass::Density(simulation.default_density))?;
            simulation.install_mass_properties(change)?;
            simulation.default_density_bodies.insert(body);
            Some(())
        })
        .is_some()
    }

    /// Change the world density and recompute the mass of every body that follows it.
    pub fn set_default_density(&self, density: f64) -> bool {
        let Some(density) = positive(density) else {
            return false;
        };
        self.update(|simulation| {
            if simulation.default_density != density {
                // Validate all derived values before changing any of the linked bodies.
                let pending = simulation
                    .default_density_bodies
                    .iter()
                    .map(|id| {
                        simulation.uniform_mass_properties(*id, UniformMass::Density(density))
                    })
                    .collect::<Option<Vec<_>>>()?;
                for change in pending {
                    simulation.install_mass_properties(change)?;
                }
                simulation.default_density = density;
                simulation.mass_changed = true;
            }
            Some(())
        })
        .is_some()
    }

    pub fn default_density(&self) -> f64 {
        self.read_mass_state(|simulation| Some(f64::from(simulation.default_density)))
            .unwrap_or_default()
    }

    /// Assign explicit local center of mass and inertia about rotated principal axes.
    /// The orientation uses the world's yaw, pitch, roll convention, in degrees.
    pub fn set_mass_properties(
        &self,
        body: i64,
        mass: f64,
        local_center: Point3D,
        principal_inertia: Vector3D,
        principal_rotation: Vector3D,
    ) -> bool {
        let Some(center) = scene_pose(local_center, principal_rotation) else {
            return false;
        };
        let Some(properties) = explicit_properties(mass, center, principal_inertia) else {
            return false;
        };
        self.set_explicit_mass(body, properties)
    }

    /// Assign a symmetric positive-definite inertia matrix in body-local coordinates.
    pub fn set_inertia_matrix(
        &self,
        body: i64,
        mass: f64,
        local_center: Point3D,
        column_x: Vector3D,
        column_y: Vector3D,
        column_z: Vector3D,
    ) -> bool {
        let Some(center) = vector([local_center.x, local_center.y, local_center.z]) else {
            return false;
        };
        let Some((moments, rotation)) = diagonalize([column_x, column_y, column_z]) else {
            return false;
        };
        let pose = Pose::from_parts(center, rotation);
        let Some(properties) = explicit_properties(mass, pose, moments) else {
            return false;
        };
        self.set_explicit_mass(body, properties)
    }

    /// The mass of a dynamic or convertible kinematic body. Fixed bodies return zero.
    pub fn mass(&self, body: i64) -> f64 {
        self.read_mass(body, |properties| f64::from(properties.mass()))
    }

    /// Read the current center of mass transformed by the body's world pose.
    pub fn center_of_mass(&self, body: i64) -> Point3D {
        self.read(body, |body| point(body.center_of_mass()))
            .unwrap_or_default()
    }

    pub fn local_center_of_mass(&self, body: i64) -> Point3D {
        self.read_mass(body, |properties| point(properties.local_com))
    }

    pub fn principal_inertia(&self, body: i64) -> Vector3D {
        self.read_mass(body, |properties| as_vector(properties.principal_inertia()))
    }

    /// Read a column (zero to two) of the inertia matrix about the local center of mass.
    pub fn inertia_column(&self, body: i64, column: i64) -> Vector3D {
        self.read_mass(body, |properties| {
            let matrix = properties.reconstruct_inertia_matrix();
            match column {
                0 => as_vector(matrix.x_axis),
                1 => as_vector(matrix.y_axis),
                2 => as_vector(matrix.z_axis),
                _ => Vector3D::default(),
            }
        })
    }

    fn set_uniform_mass(&self, body: i64, source: UniformMass) -> bool {
        self.update(|simulation| {
            let change = simulation.uniform_mass_properties(body, source)?;
            simulation.install_mass_properties(change)?;
            simulation.default_density_bodies.remove(&body);
            Some(())
        })
        .is_some()
    }

    fn set_explicit_mass(&self, body: i64, properties: MassProperties) -> bool {
        self.update(|simulation| {
            let handle = simulation.mass_handle(body)?;
            let change = simulation.prepare_mass_change(handle, properties)?;
            simulation.install_mass_properties(change)?;
            simulation.default_density_bodies.remove(&body);
            Some(())
        })
        .is_some()
    }

    fn read_mass<T: Default>(&self, body: i64, read: impl FnOnce(&MassProperties) -> T) -> T {
        self.read_mass_state(|simulation| {
            let handle = simulation.mass_handle(body)?;
            let rigid = simulation.world.bodies.get(handle)?;
            Some(read(&rigid.mass_properties().local_mprops))
        })
        .unwrap_or_default()
    }

    fn read_mass_state<T>(&self, read: impl FnOnce(&Simulation) -> Option<T>) -> Option<T> {
        let state = self.0.as_ref()?;
        state.mass_changed.track();
        let simulation = state.state.try_borrow().ok()?;
        read(&simulation)
    }
}

impl Simulation {
    pub(super) fn mass_handle(&self, body: i64) -> Option<RigidBodyHandle> {
        let handle = *self.bodies.get(&body)?;
        let rigid = self.world.bodies.get(handle)?;
        (rigid.is_dynamic()
            || (rigid.is_kinematic()
                && !self.characters.contains_key(&body)
                && self.supports_dynamic_shapes(rigid)))
        .then_some(handle)
    }

    fn uniform_mass_properties(&self, body: i64, source: UniformMass) -> Option<MassChange> {
        let handle = self.mass_handle(body)?;
        let rigid = self.world.bodies.get(handle)?;
        let mut parts = rigid.colliders().iter().filter_map(|handle| {
            let collider = self.world.colliders.get(*handle)?;
            if !collider.is_enabled() || collider.is_sensor() {
                return None;
            }
            let local_pose = collider.position_wrt_parent()?;
            Some(
                collider
                    .shape()
                    .mass_properties(1.0)
                    .transform_by(local_pose),
            )
        });
        let first = parts.next()?;
        let mut properties = parts.next().map_or(first, |second| {
            std::iter::once(first)
                .chain(std::iter::once(second))
                .chain(parts)
                .sum()
        });
        let mass = match source {
            UniformMass::Mass(mass) => mass,
            UniformMass::Density(density) => {
                positive(f64::from(density) * f64::from(properties.mass()))?
            }
        };
        properties.set_mass(mass, true);
        if properties.principal_inertia().min_element() <= 0.0 {
            return None;
        }
        self.prepare_mass_change(handle, properties)
    }

    fn prepare_mass_change(
        &self,
        body: RigidBodyHandle,
        properties: MassProperties,
    ) -> Option<MassChange> {
        let rigid = self.world.bodies.get(body)?;
        let (collider, local_pose) = rigid.colliders().iter().find_map(|&handle| {
            let collider = self.world.colliders.get(handle)?;
            if collider.is_enabled() && !collider.is_sensor() {
                Some((handle, *collider.position_wrt_parent()?))
            } else {
                None
            }
        })?;
        let properties = properties.transform_by(&local_pose.inverse());
        valid_properties(&properties).then_some(MassChange {
            body,
            collider,
            properties,
        })
    }

    fn install_mass_properties(&mut self, change: MassChange) -> Option<()> {
        let rigid = self.world.bodies.get_mut(change.body)?;
        // Store the aggregate once on an attached shape. The geometry still
        // participates in collisions, and the solver still uses its real local pose.
        self.world
            .colliders
            .get_mut(change.collider)?
            .set_mass_properties(change.properties);
        for &handle in rigid.colliders() {
            if handle != change.collider
                && let Some(collider) = self.world.colliders.get_mut(handle)
            {
                collider.set_density(0.0);
            }
        }
        rigid.recompute_mass_properties_from_colliders(&self.world.colliders);
        self.world.wake_up(change.body, true);
        self.mass_changed = true;
        Some(())
    }
}

fn point(v: Vector) -> Point3D {
    Point3D::new(v.x.into(), v.y.into(), v.z.into())
}

fn positive(value: f64) -> Option<f32> {
    let value = value as f32;
    (value > 0.0 && value.is_finite() && value.recip().is_finite()).then_some(value)
}

pub(super) fn valid_properties(properties: &MassProperties) -> bool {
    let moments = properties.principal_inertia();
    properties.local_com.is_finite()
        && properties.mass().is_finite()
        && properties.mass() > 0.0
        && moments.min_element() >= 0.0
        && moments.is_finite()
        && properties.inv_principal_inertia.is_finite()
        && properties.principal_inertia_local_frame.is_finite()
}

fn explicit_properties(mass: f64, pose: Pose, inertia: Vector3D) -> Option<MassProperties> {
    let moment = |value| {
        if value == 0.0 {
            Some(0.0)
        } else {
            positive(value)
        }
    };
    let moments = Vector::new(moment(inertia.x)?, moment(inertia.y)?, moment(inertia.z)?);
    let properties = MassProperties::with_principal_inertia_frame(
        pose.translation,
        positive(mass)?,
        moments,
        pose.rotation,
    );
    valid_properties(&properties).then_some(properties)
}

fn diagonalize(columns: [Vector3D; 3]) -> Option<(Vector3D, Rotation)> {
    let matrix = Matrix3::from_columns(&columns.map(|v| nalgebra::Vector3::new(v.x, v.y, v.z)));
    let scale = matrix.amax();
    if scale == 0.0 || !matrix.iter().all(|value| value.is_finite()) {
        return None;
    }
    let normalized = matrix / scale;
    let transpose = normalized.transpose();
    if (normalized - transpose).amax() > 1e-6 {
        return None;
    }
    let mut eigen = SymmetricEigen::try_new((normalized + transpose) * 0.5, f64::EPSILON, 64)?;
    if eigen.eigenvectors.determinant() < 0.0 {
        eigen.eigenvectors.column_mut(2).neg_mut();
    }
    let axes = [0, 1, 2].map(|index| {
        let column = eigen.eigenvectors.column(index);
        Vector::new(column.x as f32, column.y as f32, column.z as f32)
    });
    let frame = Rotation::from_mat3(&Matrix::from_cols(axes[0], axes[1], axes[2])).normalize();
    let moments = eigen.eigenvalues * scale;
    if moments.min() <= 0.0 {
        return None;
    }
    Some((Vector3D::new(moments.x, moments.y, moments.z), frame))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PhysicsCollider3D, PhysicsShape3D, VecModel};

    #[test]
    fn default_density_updates_followers_and_mass_mode_restores_geometric_inertia() {
        let origin = Point3D::default();
        let zero = Vector3D::default();
        let world = PhysicsWorld3D::new(zero, 0.01);
        let parts = VecModel::from(vec![(-1.0, 1.0), (1.0, 2.0)]);
        let shapes = PhysicsCollider3D::prepare(&parts, |&(x, width)| {
            PhysicsShape3D::new(
                "box",
                Point3D::new(x, 0.0, 0.0),
                Vector3D::new(width, 1.0, 1.0),
                [0.0; 3],
            )
        });
        let compound = world.add_body(origin, zero, shapes, 1.0, 0.0, 0.0);
        let cube = world.add_box(origin, Vector3D::new(1.0, 1.0, 1.0), 1.0, 0.0, 0.0);
        assert!(compound > 0 && cube > 0);
        assert!(world.use_default_density(compound));
        assert!(world.use_default_density(cube));
        assert!((world.mass(compound) - 3.0).abs() < 1e-5);
        assert!((world.local_center_of_mass(compound).x - 1.0 / 3.0).abs() < 1e-5);
        let before = world.inertia_column(compound, 0);
        assert!(!world.set_default_density(2e38));
        assert_eq!(world.default_density(), 1.0);
        assert_eq!(world.mass(cube), 1.0);
        assert!((world.mass(compound) - 3.0).abs() < 1e-5);
        assert!(world.set_default_density(2.0));
        assert!((world.mass(compound) - 6.0).abs() < 1e-5);
        assert!((world.mass(cube) - 2.0).abs() < 1e-5);
        assert!((world.inertia_column(compound, 0).x - before.x * 2.0).abs() < 1e-5);

        assert!(world.set_density(cube, 5.0));
        assert!(world.set_mass_properties(
            compound,
            4.0,
            Point3D::new(0.0, 0.8, 0.0),
            Vector3D::new(1.0, 2.0, 3.0),
            zero,
        ));
        assert_eq!(world.mass(compound), 4.0);
        assert!((world.local_center_of_mass(compound).y - 0.8).abs() < 1e-6);
        assert!((world.principal_inertia(compound).z - 3.0).abs() < 1e-6);
        assert!(world.set_mass(compound, 8.0));
        assert!(world.set_default_density(3.0));
        assert_eq!(world.mass(compound), 8.0);
        assert_eq!(world.mass(cube), 5.0);
        assert!((world.local_center_of_mass(compound).x - 1.0 / 3.0).abs() < 1e-5);
        assert!(world.use_default_density(cube));
        assert!(world.remove(cube));
        assert!(world.clear());
        assert!(world.set_default_density(4.0));
        assert_eq!(world.default_density(), 4.0);
    }

    #[test]
    fn explicit_center_and_matrix_apply_to_the_very_next_impulse() {
        let zero = Vector3D::default();
        let world = PhysicsWorld3D::new(zero, 0.01);
        let position = Point3D::new(3.0, 4.0, 5.0);
        let center = Point3D::new(0.0, 1.0, 0.0);
        let body = world.add_box(position, Vector3D::new(1.0, 1.0, 1.0), 1.0, 0.0, 0.0);
        let [a, b, c] = [
            Vector3D::new(2.0, 1.0, 0.0),
            Vector3D::new(1.0, 2.0, 0.0),
            Vector3D::new(0.0, 0.0, 4.0),
        ];
        assert!(world.set_inertia_matrix(body, 2.0, center, a, b, c));
        let effective_center = world.center_of_mass(body);
        assert_eq!(effective_center, Point3D::new(3.0, 5.0, 5.0));
        assert_eq!(world.local_center_of_mass(body), center);
        let read_back = world.inertia_column(body, 0);
        assert!((read_back.x - 2.0).abs() < 1e-5 && (read_back.y - 1.0).abs() < 1e-5);
        assert!(!world.set_inertia_matrix(body, 6.0, center, a, Vector3D::new(0.0, 2.0, 0.0), c));
        assert!(!world.set_inertia_matrix(body, 6.0, center, a, b, Vector3D::new(0.0, 0.0, -4.0)));
        assert_eq!(world.mass(body), 2.0);
        assert!(world.apply_impulse_at(body, Vector3D::new(2.0, 0.0, 0.0), effective_center));
        assert_eq!(world.velocity(body), Vector3D::new(1.0, 0.0, 0.0));
        assert_eq!(world.angular_velocity(body), zero);
        assert!(world.apply_torque_impulse(body, Vector3D::new(1.0, 0.0, 0.0)));
        let angular = world.angular_velocity(body);
        assert!((angular.x - 2.0 / 3.0).abs() < 1e-5, "{angular:?}");
        assert!((angular.y + 1.0 / 3.0).abs() < 1e-5, "{angular:?}");
        assert!(world.step());
        assert_eq!(world.mass(body), 2.0);
        assert_eq!(world.local_center_of_mass(body), center);
    }
}
