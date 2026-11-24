//! Continuous sphere queries against posed SceneView3D collision geometry.
use nalgebra::Vector3;

use crate::{
    VecModel, Vector3D,
    scene_camera::Point3D,
    scene_hierarchy::SceneModel,
    scene_pick::BoundsSegment,
    scene3d::{Hit3D, Hit3DData, MAIN_LAYER, SceneObject},
};

#[derive(Clone, Copy)]
pub(crate) struct SphereSweep {
    pub origin: Vector3<f64>,
    pub direction: Vector3<f64>,
    pub radius: f64,
    distance: f64,
    blocking_only: bool,
}

impl SphereSweep {
    fn new(origin: Point3D, direction: Vector3D, distance: f64, radius: f64) -> Option<Self> {
        // Use the scene's representable coordinate range, but solve contacts in f64.
        origin.narrow()?;
        if !(distance >= 0.0
            && (distance as f32).is_finite()
            && radius > 0.0
            && (radius as f32).is_finite()
            && radius * radius > 0.0)
        {
            return None;
        }
        let direction = Vector3::new(direction.x, direction.y, direction.z);
        let scale = direction.abs().max();
        if !direction.iter().copied().all(f64::is_finite) || scale == 0.0 {
            return None;
        }
        let direction = (direction / scale).try_normalize(0.0)?;
        Some(Self {
            origin: Vector3::new(origin.x, origin.y, origin.z),
            direction,
            radius,
            distance,
            blocking_only: false,
        })
    }

    pub(crate) fn span(self, max: f64) -> BoundsSegment {
        BoundsSegment {
            origin: self.origin,
            direction: self.direction,
            min: 0.0,
            max,
            expansion: Vector3::repeat(self.radius),
        }
    }
}

pub(crate) struct MeshContact {
    pub contact: TriangleContact,
    pub local_position: Vector3<f64>,
    pub local_normal: Vector3<f64>,
    pub uv: [f64; 2],
    pub inside: bool,
}

impl MeshContact {
    fn describe(self, object: &SceneObject) -> Hit3D {
        let [x, y, z] = self.contact.location.into();
        let [nx, ny, nz] = self.contact.normal.into();
        let [lx, ly, lz] = self.local_position.into();
        let [lnx, lny, lnz] = self.local_normal.into();
        Hit3D::from_data(Hit3DData {
            valid: true,
            object_id: object.id.clone(),
            row_index: object.scene_row().map_or(-1, i64::from),
            mesh_node_index: object
                .imported_placement()
                .map_or(-1, |(node, _)| i64::from(node)),
            instance_index: object
                .imported_placement()
                .and_then(|(_, instance)| instance)
                .map_or(-1, i64::from),
            distance: self.contact.distance,
            position: Point3D::new(x, y, z),
            normal: Vector3D::new(nx, ny, nz),
            local_position: Point3D::new(lx, ly, lz),
            local_normal: Vector3D::new(lnx, lny, lnz),
            u: self.uv[0],
            v: self.uv[1],
        })
    }
}

/// Return the first contact along the swept center path, in world units.
#[doc(hidden)]
pub fn sphere_cast(
    model: &SceneModel,
    origin: Point3D,
    direction: Vector3D,
    distance: f64,
    radius: f64,
    layers: Option<i64>,
) -> Hit3D {
    let Some(sweep) = SphereSweep::new(origin, direction, distance, radius) else {
        return Hit3D::default();
    };
    let layers = layers.map_or(MAIN_LAYER, |value| value as u64);
    closest_sweep_contact(model, sweep, layers)
        .map_or_else(Hit3D::default, |(index, hit)| hit.describe(&model[index]))
}

fn closest_sweep_contact(
    model: &SceneModel,
    sweep: SphereSweep,
    layers: u64,
) -> Option<(usize, MeshContact)> {
    if layers == 0 {
        return None;
    }
    let mut closest: Option<(usize, MeshContact)> = None;
    model.visit_sweep(sweep.span(sweep.distance), |index, max| {
        let object = &model[index];
        if object.layers & layers == 0 {
            return max;
        }
        if let Some(found) = object.sphere_contact(sweep, max)
            && closest.as_ref().is_none_or(|(previous_index, previous)| {
                found
                    .contact
                    .distance
                    .total_cmp(&previous.contact.distance)
                    .then_with(|| {
                        if sweep.blocking_only && found.inside != previous.inside {
                            previous.inside.cmp(&found.inside)
                        } else {
                            index.cmp(previous_index)
                        }
                    })
                    .is_lt()
            })
        {
            let distance = found.contact.distance;
            closest = Some((index, found));
            distance
        } else {
            max
        }
    });
    closest
}

const MAX_SLIDE_CONTACTS: usize = 8;

/// Resolve an intended displacement against scene colliders and return the sphere center.
#[doc(hidden)]
pub fn sphere_slide(
    model: &SceneModel,
    origin: Point3D,
    displacement: Vector3D,
    radius: f64,
    layers: Option<i64>,
) -> Point3D {
    let remaining = world_vector(displacement);
    let Some(sweep) = movement_sweep(origin, displacement, radius) else {
        return origin;
    };
    let layers = layers.map_or(MAIN_LAYER, |value| value as u64);
    world_point(solve_motion(model, sweep, remaining, layers, None).position)
}

/// Options for the grounded sphere controller.
#[doc(hidden)]
pub struct WalkOptions {
    pub radius: f64,
    pub step_height: f64,
    pub snap_distance: f64,
    pub max_slope: f64,
}

/// Move a sphere with slope limits, optional step-up, and ground following.
#[doc(hidden)]
pub fn sphere_walk(
    model: &SceneModel,
    origin: Point3D,
    displacement: Vector3D,
    options: WalkOptions,
    layers: Option<i64>,
) -> Point3D {
    let requested = world_vector(displacement);
    let Some(sweep) = movement_sweep(origin, displacement, options.radius) else {
        return origin;
    };
    let valid_distance = |distance: f64| distance >= 0.0 && (distance as f32).is_finite();
    if !valid_distance(options.step_height)
        || !valid_distance(options.snap_distance)
        || !(0.0..=90.0).contains(&options.max_slope)
    {
        return origin;
    }
    let min_normal_y = options.max_slope.to_radians().cos();
    let layers = layers.map_or(MAIN_LAYER, |value| value as u64);
    if layers == 0 {
        return world_point(sweep.origin + requested);
    }
    let walk = GroundMotion {
        model,
        sweep,
        layers,
        min_normal_y,
        skin: sweep_skin(sweep.radius),
    };
    let grounded = requested.y <= 0.0
        && (options.step_height > 0.0 || options.snap_distance > 0.0)
        && walk.land(sweep.origin, 2.0 * walk.skin).is_some();
    let ordinary = solve_motion(model, sweep, requested, layers, Some(min_normal_y));

    if grounded {
        if ordinary.blocked
            && options.step_height > 0.0
            && let Some(stepped) = walk.step(requested, ordinary.position, &options)
        {
            return world_point(stepped);
        }
        if options.snap_distance > 0.0
            && let Some(landed) = walk.land(ordinary.position, options.snap_distance)
        {
            return world_point(landed);
        }
    }
    world_point(ordinary.position)
}

struct GroundMotion<'a> {
    model: &'a SceneModel,
    sweep: SphereSweep,
    layers: u64,
    min_normal_y: f64,
    skin: f64,
}

impl GroundMotion<'_> {
    fn vertical_contact(
        &self,
        position: Vector3<f64>,
        direction: f64,
        distance: f64,
    ) -> Option<MeshContact> {
        closest_sweep_contact(
            self.model,
            SphereSweep {
                origin: position,
                direction: Vector3::y() * direction,
                distance,
                blocking_only: true,
                ..self.sweep
            },
            self.layers,
        )
        .map(|(_, contact)| contact)
    }

    fn land(&self, position: Vector3<f64>, distance: f64) -> Option<Vector3<f64>> {
        let hit = self.vertical_contact(position, -1.0, distance)?;
        (!hit.inside && hit.contact.normal.y >= self.min_normal_y)
            .then(|| position - Vector3::y() * (hit.contact.distance - self.skin).max(0.0))
    }

    fn step(
        &self,
        requested: Vector3<f64>,
        ordinary: Vector3<f64>,
        options: &WalkOptions,
    ) -> Option<Vector3<f64>> {
        let horizontal = Vector3::new(requested.x, 0.0, requested.z);
        let direction = horizontal.try_normalize(0.0)?;
        let origin = self.sweep.origin;
        let rise = self
            .vertical_contact(origin, 1.0, options.step_height)
            .map_or(options.step_height, |hit| {
                (hit.contact.distance - self.skin).max(0.0)
            });
        if rise <= self.skin {
            return None;
        }
        let raised = origin + rise * Vector3::y();
        let crossed = solve_motion(
            self.model,
            SphereSweep {
                origin: raised,
                ..self.sweep
            },
            horizontal,
            self.layers,
            Some(self.min_normal_y),
        )
        .position;
        if (crossed - ordinary).dot(&direction) <= self.skin {
            return None;
        }
        let landed = self.land(crossed, rise + options.snap_distance + self.skin)?;
        let height_gain = landed.y - origin.y;
        (height_gain <= options.step_height + self.skin
            && height_gain >= -options.snap_distance - self.skin)
            .then_some(landed)
    }
}

fn world_point(value: Vector3<f64>) -> Point3D {
    Point3D::new(value.x, value.y, value.z)
}

fn world_vector(value: Vector3D) -> Vector3<f64> {
    Vector3::new(value.x, value.y, value.z)
}

fn movement_sweep(origin: Point3D, displacement: Vector3D, radius: f64) -> Option<SphereSweep> {
    let vector = world_vector(displacement);
    let sweep = SphereSweep::new(origin, displacement, vector.norm(), radius)?;
    (sweep.origin + vector)
        .iter()
        .all(|&component| (component as f32).is_finite())
        .then_some(sweep)
}

fn sweep_skin(radius: f64) -> f64 {
    radius * 0.0001
}

struct MoveResult {
    position: Vector3<f64>,
    blocked: bool,
}

fn solve_motion(
    model: &SceneModel,
    mut sweep: SphereSweep,
    mut remaining: Vector3<f64>,
    layers: u64,
    min_normal_y: Option<f64>,
) -> MoveResult {
    let mut position = sweep.origin;
    let mut blocked = false;
    // Steep upward-facing contacts can contribute both their real normal and
    // a horizontal barrier, preventing horizontal input from climbing them.
    let mut planes = [Vector3::zeros(); 2 * MAX_SLIDE_CONTACTS];
    let mut plane_count = 0;
    let skin = sweep_skin(sweep.radius);
    sweep.blocking_only = true;
    for _ in 0..MAX_SLIDE_CONTACTS {
        let distance = remaining.norm();
        if distance == 0.0 {
            break;
        }
        sweep.origin = position;
        sweep.direction = remaining / distance;
        sweep.distance = distance;
        let Some((_, hit)) = closest_sweep_contact(model, sweep, layers) else {
            position += remaining;
            break;
        };
        if hit.inside {
            break;
        }
        let advance = (hit.contact.distance - skin).max(0.0);
        let moved = sweep.direction * advance;
        position += moved;
        remaining -= moved;

        let normal = hit.contact.normal;
        let mut add_plane = |normal: Vector3<f64>| {
            if !planes[..plane_count]
                .iter()
                .any(|plane| plane.dot(&normal) > 1.0 - 1e-10)
            {
                planes[plane_count] = normal;
                plane_count += 1;
            }
        };
        add_plane(normal);
        if min_normal_y.is_some_and(|minimum| normal.y < minimum) {
            blocked = true;
            if normal.y > 0.0
                && let Some(horizontal) = Vector3::new(normal.x, 0.0, normal.z).try_normalize(1e-12)
            {
                add_plane(horizontal);
            }
        }
        let sliding = project_motion(remaining, &planes[..plane_count]);
        if advance == 0.0 && sliding == remaining {
            break;
        }
        remaining = sliding;
    }
    MoveResult { position, blocked }
}

// The nearest feasible displacement is the input, a plane projection, a
// projection onto two intersecting planes, or the cone apex at zero.
fn project_motion(motion: Vector3<f64>, planes: &[Vector3<f64>]) -> Vector3<f64> {
    let tolerance = motion.norm() * 1e-12;
    let feasible = |candidate: Vector3<f64>| {
        planes
            .iter()
            .all(|plane| candidate.dot(plane) >= -tolerance)
    };
    if feasible(motion) {
        return motion;
    }
    let mut best = Vector3::zeros();
    let mut best_error = motion.norm_squared();
    let mut consider = |candidate: Vector3<f64>| {
        let error = (candidate - motion).norm_squared();
        if error < best_error && feasible(candidate) {
            best = candidate;
            best_error = error;
        }
    };
    for (index, normal) in planes.iter().enumerate() {
        consider(motion - normal * motion.dot(normal));
        for other in &planes[..index] {
            let crease = normal.cross(other);
            let length = crease.norm_squared();
            if length > 1e-24 {
                consider(crease * (motion.dot(&crease) / length));
            }
        }
    }
    best
}

/// Return the nearest contact of each overlapping authored row, in row order.
#[doc(hidden)]
pub fn sphere_overlap(
    model: &SceneModel,
    center: Point3D,
    radius: f64,
    layers: Option<i64>,
) -> VecModel<Hit3D> {
    let Some(sweep) = SphereSweep::new(center, Vector3D::new(0.0, 1.0, 0.0), 0.0, radius) else {
        return VecModel::default();
    };
    let layers = layers.map_or(MAIN_LAYER, |value| value as u64);
    if layers == 0 {
        return VecModel::default();
    }
    let mut contacts = Vec::new();
    model.visit_sweep(sweep.span(0.0), |index, max| {
        let object = &model[index];
        if object.layers & layers != 0
            && let Some(hit) = object.sphere_contact(sweep, max)
        {
            contacts.push((index, hit));
        }
        max
    });
    // A BVH traverses in spatial order. Restore the model order and keep one
    // contact per authored row even when a glTF row expands into several meshes.
    contacts.sort_unstable_by_key(|(index, _)| *index);
    let mut results = Vec::new();
    let mut row: Option<(usize, MeshContact)> = None;
    for (index, hit) in contacts {
        if let Some((previous_index, previous)) = row.as_mut()
            && model[index].scene_row().is_some()
            && model[index].scene_row() == model[*previous_index].scene_row()
        {
            if hit.contact.initial_gap < previous.contact.initial_gap {
                *previous_index = index;
                *previous = hit;
            }
        } else if let Some((previous_index, previous)) = row.replace((index, hit)) {
            results.push(previous.describe(&model[previous_index]));
        }
    }
    if let Some((index, hit)) = row {
        results.push(hit.describe(&model[index]));
    }
    results.into()
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TriangleContact {
    pub distance: f64,
    pub location: Vector3<f64>,
    pub normal: Vector3<f64>,
    pub weights: [f64; 3],
    pub initial_gap: f64,
}

struct Triangle {
    points: [Vector3<f64>; 3],
    normal: Option<Vector3<f64>>,
}

impl Triangle {
    fn new(points: [Vector3<f64>; 3]) -> Self {
        let [a, b, c] = points;
        Self {
            points,
            normal: (b - a).cross(&(c - a)).try_normalize(0.0),
        }
    }

    fn weights(&self, point: Vector3<f64>, normal: Vector3<f64>) -> Option<[f64; 3]> {
        let [a, b, c] = self.points;
        let ab = b - a;
        let ac = c - a;
        let offset = point - a;
        let area = ab.cross(&ac).dot(&normal);
        let u = offset.cross(&ac).dot(&normal) / area;
        let v = ab.cross(&offset).dot(&normal) / area;
        let w = 1.0 - u - v;
        [w, u, v]
            .into_iter()
            .all(|weight| (0.0..=1.0).contains(&weight))
            .then_some([w, u, v])
    }

    fn edges(&self) -> impl Iterator<Item = (usize, Vector3<f64>, Vector3<f64>)> + '_ {
        self.points.iter().copied().enumerate().map(|(index, a)| {
            let edge = self.points[(index + 1) % 3] - a;
            (index, a, edge)
        })
    }

    fn closest(&self, center: Vector3<f64>) -> (Vector3<f64>, [f64; 3]) {
        if let Some(normal) = self.normal {
            let surface = center - normal * normal.dot(&(center - self.points[0]));
            if let Some(weights) = self.weights(surface, normal) {
                return (surface, weights);
            }
        }
        let mut closest = (self.points[0], [1.0, 0.0, 0.0]);
        let mut distance = (center - closest.0).norm_squared();
        for (index, a, edge) in self.edges() {
            let length = edge.norm_squared();
            if length > 0.0 {
                let from_a = ((center - a).dot(&edge) / length).clamp(0.0, 1.0);
                let point = a + edge * from_a;
                let gap = (center - point).norm_squared();
                if gap < distance {
                    distance = gap;
                    closest = (point, edge_weights(index, from_a));
                }
            }
        }
        closest
    }
}

fn edge_weights(index: usize, along: f64) -> [f64; 3] {
    let mut weights = [0.0; 3];
    weights[index] = 1.0 - along;
    weights[(index + 1) % 3] = along;
    weights
}

// Earliest entering root of a*t² + 2*b*t + c, avoiding cancellation
// near contact. Initial intersections are handled by closest-point tests.
fn entering_root(a: f64, b: f64, c: f64, max: f64) -> Option<f64> {
    if a <= 0.0 || b >= 0.0 || c < 0.0 {
        return None;
    }
    let discriminant = b.mul_add(b, -a * c);
    if discriminant < 0.0 {
        return None;
    }
    let t = c / (-b + discriminant.sqrt());
    (0.0..=max).contains(&t).then_some(t)
}

pub(crate) fn cast_triangle(
    sweep: SphereSweep,
    points: [Vector3<f64>; 3],
    inside: bool,
    mut max: f64,
) -> Option<TriangleContact> {
    let triangle = Triangle::new(points);
    let (location, weights) = triangle.closest(sweep.origin);
    let delta = sweep.origin - location;
    let gap = delta.norm_squared();
    let r2 = sweep.radius * sweep.radius;
    if inside || gap <= r2 * (1.0 + 1e-10) {
        let normal = delta.try_normalize(0.0).map_or_else(
            || {
                triangle.normal.map_or(-sweep.direction, |normal| {
                    if normal.dot(&sweep.direction) > 0.0 {
                        -normal
                    } else {
                        normal
                    }
                })
            },
            |normal| if inside { -normal } else { normal },
        );
        let approaching =
            sweep.direction.dot(&normal) < if sweep.blocking_only { -1e-8 } else { 0.0 };
        if inside
            || sweep.distance == 0.0
            || approaching
            || !sweep.blocking_only && gap < r2 * (1.0 - 1e-10)
        {
            return Some(TriangleContact {
                distance: 0.0,
                location,
                normal,
                weights,
                initial_gap: gap,
            });
        }
    }
    if max == 0.0 {
        return None;
    }
    let mut nearest = None;
    let mut accept = |distance: f64, point, weights, normal, max: &mut f64| {
        if sweep.blocking_only && sweep.direction.dot(&normal) >= -1e-8 {
            return;
        }
        if nearest.is_none() || distance < *max {
            nearest = Some(TriangleContact {
                distance,
                location: point,
                normal,
                weights,
                initial_gap: 0.0,
            });
            *max = distance;
        }
    };
    if let Some(mut normal) = triangle.normal {
        if normal.dot(&sweep.direction) > 0.0 {
            normal = -normal;
        }
        let toward = -normal.dot(&sweep.direction);
        if toward > 0.0 {
            let plane = (sweep.origin - points[0]).dot(&normal);
            let distance = (plane - sweep.radius) / toward;
            if (0.0..=max).contains(&distance) {
                let point = sweep.origin + sweep.direction * distance - normal * sweep.radius;
                if let Some(weights) = triangle.weights(point, normal) {
                    accept(distance, point, weights, normal, &mut max);
                }
            }
        }
    }
    for (index, a, edge) in triangle.edges() {
        let length = edge.norm_squared();
        if length == 0.0 {
            continue;
        }
        let start = sweep.origin - a;
        let s = start.dot(&edge) / length;
        let rate = sweep.direction.dot(&edge) / length;
        let radial = start - edge * s;
        let velocity = sweep.direction - edge * rate;
        if let Some(distance) = entering_root(
            velocity.norm_squared(),
            radial.dot(&velocity),
            radial.norm_squared() - r2,
            max,
        ) {
            let along = s + rate * distance;
            if (0.0..=1.0).contains(&along)
                && let Some(normal) = (radial + velocity * distance).try_normalize(0.0)
            {
                accept(
                    distance,
                    a + edge * along,
                    edge_weights(index, along),
                    normal,
                    &mut max,
                );
            }
        }
    }
    for (index, point) in points.into_iter().enumerate() {
        let delta = sweep.origin - point;
        if let Some(distance) = entering_root(
            sweep.direction.norm_squared(),
            delta.dot(&sweep.direction),
            delta.norm_squared() - r2,
            max,
        ) && let Some(normal) = (delta + sweep.direction * distance).try_normalize(0.0)
        {
            accept(distance, point, edge_weights(index, 0.0), normal, &mut max);
        }
    }
    nearest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sweep(start: [f64; 3], direction: [f64; 3], radius: f64) -> SphereSweep {
        let [x, y, z] = start;
        let [dx, dy, dz] = direction;
        SphereSweep::new(
            Point3D::new(x, y, z),
            Vector3D::new(dx, dy, dz),
            10.0,
            radius,
        )
        .expect("finite test sweep")
    }

    #[test]
    fn sphere_intersects_faces_edges_and_isolated_vertices() {
        let triangle = [
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 0.0),
            Vector3::new(0.0, 2.0, 0.0),
        ];
        let face = cast_triangle(
            sweep([0.5, 0.5, 5.0], [0.0, 0.0, -2.0], 1.0),
            triangle,
            false,
            10.0,
        )
        .expect("face contact");
        assert_eq!(face.distance, 4.0);
        assert_eq!(face.normal, Vector3::z());
        assert_eq!(face.location, Vector3::new(0.5, 0.5, 0.0));
        let edge = cast_triangle(
            sweep([0.5, -0.6, 5.0], [0.0, 0.0, -1.0], 1.0),
            triangle,
            false,
            10.0,
        )
        .expect("the center ray misses; the sphere touches the edge");
        assert!((edge.distance - 4.2).abs() < 1e-12);
        assert_eq!(edge.location, Vector3::new(0.5, 0.0, 0.0));
        let corner = cast_triangle(
            sweep([-0.6, -0.6, 5.0], [0.0, 0.0, -1.0], 1.0),
            triangle,
            false,
            10.0,
        )
        .expect("corner contact");
        assert!((corner.distance - (5.0 - 0.28_f64.sqrt())).abs() < 1e-12);
        assert_eq!(corner.location, Vector3::zeros());
        // Degenerate point triangles still provide a deterministic tangent.
        let tangent = cast_triangle(
            sweep([-5.0, 1.0, 0.0], [1.0, 0.0, 0.0], 1.0),
            [Vector3::zeros(); 3],
            false,
            10.0,
        )
        .expect("tangent");
        assert_eq!(tangent.distance, 5.0);
        assert_eq!(tangent.normal, Vector3::y());
    }

    #[test]
    fn motion_queries_pass_tangent_mesh_edges_and_clip_into_the_crease() {
        let [a, b, c, d] = [
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(4.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 4.0),
            Vector3::new(4.0, 0.0, 4.0),
        ];
        let mut motion = sweep([-2.0, 1.0, 1.25], [1.0, 0.0, 0.0], 1.0);
        assert!(cast_triangle(motion, [a, b, c], false, 10.0).is_some());
        motion.blocking_only = true;
        for triangle in [[a, b, c], [b, d, c]] {
            assert!(cast_triangle(motion, triangle, false, 10.0).is_none());
        }

        let planes = [-Vector3::x(), Vector3::new(0.6, 0.0, -0.8)];
        let desired = Vector3::new(2.0, 1.0, 3.0);
        let constrained = project_motion(desired, &planes);
        assert!((constrained - Vector3::y()).norm() < 1e-12);
        let separating = Vector3::new(-1.0, 0.0, -2.0);
        assert_eq!(project_motion(separating, &planes), separating);
    }

    #[test]
    fn exact_touching_allows_sliding_and_separating_but_detects_overlap() {
        let triangle = [
            Vector3::new(-8.0, -8.0, 0.0),
            Vector3::new(8.0, -8.0, 0.0),
            Vector3::new(0.0, 8.0, 0.0),
        ];
        for direction in [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0]] {
            assert!(
                cast_triangle(sweep([0.0, 0.0, 1.0], direction, 1.0), triangle, false, 2.0)
                    .is_none()
            );
        }
        let penetrating = cast_triangle(
            sweep([0.0, 0.0, 0.8], [0.0, 0.0, 1.0], 1.0),
            triangle,
            false,
            2.0,
        )
        .expect("initial overlap");
        assert_eq!(penetrating.distance, 0.0);
        assert_eq!(penetrating.normal, Vector3::z());
        let inward = cast_triangle(
            sweep([0.0, 0.0, 1.0], [0.0, 0.0, -1.0], 1.0),
            triangle,
            false,
            2.0,
        )
        .expect("immediate inward contact");
        assert_eq!(inward.distance, 0.0);
    }
}

#[cfg(test)]
mod scene_tests {
    use super::*;
    use crate::{Color, scene_visibility::VisibilityIndex};

    #[test]
    fn sphere_inside_a_builtin_solid_gets_an_exit_contact() {
        let sweep = SphereSweep::new(Point3D::default(), Vector3D::new(1.0, 0.0, 0.0), 3.0, 0.05)
            .expect("test sweep");
        let box_object =
            SceneObject::cuboid("box", [0.0; 3], [1.0; 3], [0.0; 3], Color(0xffffffff))
                .expect("box");
        let sphere = SceneObject::sphere("ball", [0.0; 3], 1.0, Color(0xffffffff)).expect("sphere");
        for object in [box_object, sphere] {
            let hit = object
                .sphere_contact(sweep, 3.0)
                .expect("inside a convex solid");
            assert_eq!(hit.contact.distance, 0.0);
            assert!(hit.contact.location.dot(&hit.contact.normal) >= 0.49);
        }
    }

    #[test]
    fn scene_bvh_expands_by_radius_and_prunes_distant_rows() {
        let mut objects = Vec::new();
        for x in -16..=16 {
            for z in -16..=16 {
                objects.push(
                    SceneObject::cuboid(
                        "building",
                        [x as f32 * 3.0, 0.0, z as f32 * 3.0],
                        [1.0; 3],
                        [0.0; 3],
                        Color(0xffffffff),
                    )
                    .expect("grid box"),
                );
            }
        }
        let sweep = SphereSweep::new(
            Point3D::new(-49.5, 0.0, 0.9),
            Vector3D::new(1.0, 0.0, 0.0),
            10.0,
            0.55,
        )
        .expect("test sweep");
        let tree = VisibilityIndex::new(&objects).expect("scene index");
        let mut visits = 0;
        let mut contact = None;
        tree.visit_sweep(sweep.span(10.0), |index, max| {
            visits += 1;
            if let Some(hit) = objects[index].sphere_contact(sweep, max) {
                let distance = hit.contact.distance;
                contact = Some(distance);
                distance
            } else {
                max
            }
        });
        assert!(
            (contact.expect("radius reaches a row missed by the center line")
                - (1.0 - 0.1425_f64.sqrt()))
            .abs()
                < 1e-10
        );
        assert!(
            visits < 24,
            "{visits} of {} scene objects visited",
            objects.len()
        );
    }
}
