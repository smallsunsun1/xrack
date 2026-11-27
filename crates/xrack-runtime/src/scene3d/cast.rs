//! Continuous sphere and upright capsule queries against posed SceneView3D geometry.
use nalgebra::Vector3;

use crate::{
    VecModel, Vector3D,
    scene3d::camera::Point3D,
    scene3d::hierarchy::SceneModel,
    scene3d::pick::BoundsSegment,
    scene3d::{Hit3D, Hit3DData, MAIN_LAYER, SceneObject},
};

#[derive(Clone, Copy)]
pub(crate) struct ShapeSweep {
    pub origin: Vector3<f64>,
    pub direction: Vector3<f64>,
    pub radius: f64,
    pub half_segment: f64,
    distance: f64,
    blocking_only: bool,
}

impl ShapeSweep {
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
            half_segment: 0.0,
            distance,
            blocking_only: false,
        })
    }

    fn with_segment_height(mut self, height: f64) -> Option<Self> {
        if height < 0.0 || !(height as f32).is_finite() {
            return None;
        }
        self.half_segment = height * 0.5;
        Some(self)
    }

    pub(crate) fn span(self, max: f64) -> BoundsSegment {
        BoundsSegment {
            origin: self.origin,
            direction: self.direction,
            min: 0.0,
            max,
            expansion: Vector3::repeat(self.radius) + self.half_segment * Vector3::y(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct MeshContact {
    pub contact: TriangleContact,
    pub local_position: Vector3<f64>,
    pub local_normal: Vector3<f64>,
    pub uv: [f64; 2],
    pub inside: bool,
}

impl MeshContact {
    fn describe(self, object: &SceneObject) -> Hit3D {
        self.describe_contact(object, false)
    }

    fn describe_contact(self, object: &SceneObject, entering: bool) -> Hit3D {
        let [x, y, z] = self.contact.location.into();
        let [nx, ny, nz] = self.contact.normal.into();
        let [lx, ly, lz] = self.local_position.into();
        let [lnx, lny, lnz] = self.local_normal.into();
        Hit3D::from_data(Hit3DData {
            valid: true,
            entering,
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
            item: None,
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
    capsule_cast(model, origin, direction, distance, radius, 0.0, layers)
}

/// Sweep a vertical capsule. Segment height is measured between the hemisphere centers.
#[doc(hidden)]
pub fn capsule_cast(
    model: &SceneModel,
    origin: Point3D,
    direction: Vector3D,
    distance: f64,
    radius: f64,
    segment_height: f64,
    layers: Option<i64>,
) -> Hit3D {
    let Some(sweep) = ShapeSweep::new(origin, direction, distance, radius)
        .and_then(|sweep| sweep.with_segment_height(segment_height))
    else {
        return Hit3D::default();
    };
    let layers = layers.map_or(MAIN_LAYER, |value| value as u64);
    closest_sweep_contact(model, sweep, layers).map_or_else(Hit3D::default, |(index, hit)| {
        hit.describe(&model.collision_geometry()[index])
    })
}

fn closest_sweep_contact(
    model: &SceneModel,
    sweep: ShapeSweep,
    layers: u64,
) -> Option<(usize, MeshContact)> {
    if layers == 0 {
        return None;
    }
    let mut closest: Option<(usize, MeshContact)> = None;
    model.visit_sweep(sweep.span(sweep.distance), |index, max| {
        let object = &model.collision_geometry()[index];
        if object.layers & layers == 0 {
            return max;
        }
        if let Some(found) = object.sweep_contact(sweep, max)
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
    capsule_slide(model, origin, displacement, radius, 0.0, layers)
}

/// Move a vertical capsule while sliding against blocking contacts.
#[doc(hidden)]
pub fn capsule_slide(
    model: &SceneModel,
    origin: Point3D,
    displacement: Vector3D,
    radius: f64,
    segment_height: f64,
    layers: Option<i64>,
) -> Point3D {
    let remaining = world_vector(displacement);
    let Some(sweep) = movement_sweep(origin, displacement, radius, segment_height) else {
        return origin;
    };
    let layers = layers.map_or(MAIN_LAYER, |value| value as u64);
    world_point(
        solve_motion::<false, false, false>(
            model,
            sweep,
            remaining,
            layers,
            None,
            &mut MotionPath::<false>::default(),
        )
        .position,
    )
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
    capsule_walk(model, origin, displacement, 0.0, options, layers)
}

/// Move a vertical capsule with slope limits, step-up, and ground following.
#[doc(hidden)]
pub fn capsule_walk(
    model: &SceneModel,
    origin: Point3D,
    displacement: Vector3D,
    segment_height: f64,
    options: WalkOptions,
    layers: Option<i64>,
) -> Point3D {
    world_point(
        walk::<false, false, false>(
            model,
            origin,
            displacement,
            segment_height,
            &options,
            layers,
            &mut MotionPath::default(),
        )
        .position,
    )
}

/// A resolved capsule movement and its trigger crossings in travel order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WalkResult3D {
    pub position: Point3D,
    pub crossings: VecModel<Hit3D>,
    #[doc(hidden)]
    pub collision_contacts: Option<VecModel<Hit3D>>,
    /// The final walkable support for a detailed walk, including an idle probe.
    pub support: Hit3D,
    pub grounded: bool,
    pub hit_wall: bool,
    pub hit_ceiling: bool,
}

impl WalkResult3D {
    /// Blocking surface contacts in the order they occurred.
    pub fn contacts(&self) -> VecModel<Hit3D> {
        self.collision_contacts.clone().unwrap_or_default()
    }
}

/// Resolve a capsule walk and collect trigger crossings from the accepted path.
#[doc(hidden)]
pub fn capsule_walk_result(
    model: &SceneModel,
    origin: Point3D,
    displacement: Vector3D,
    segment_height: f64,
    options: WalkOptions,
    layers: Option<i64>,
) -> WalkResult3D {
    walk_result::<false>(model, origin, displacement, segment_height, options, layers)
}

/// Resolve a capsule walk with blocking surface contacts as well as trigger crossings.
#[doc(hidden)]
pub fn capsule_walk_with_contacts(
    model: &SceneModel,
    origin: Point3D,
    displacement: Vector3D,
    segment_height: f64,
    options: WalkOptions,
    layers: Option<i64>,
) -> WalkResult3D {
    walk_result::<true>(model, origin, displacement, segment_height, options, layers)
}

fn walk_result<const CONTACTS: bool>(
    model: &SceneModel,
    origin: Point3D,
    displacement: Vector3D,
    segment_height: f64,
    options: WalkOptions,
    layers: Option<i64>,
) -> WalkResult3D {
    let mask = layers.map_or(MAIN_LAYER, |value| value as u64);
    let mut path = MotionPath {
        enabled: displacement != Vector3D::default() && model.has_triggers(mask),
        points: Vec::new(),
    };
    let movement = walk::<true, true, CONTACTS>(
        model,
        origin,
        displacement,
        segment_height,
        &options,
        layers,
        &mut path,
    );
    let mut events = Vec::new();
    let mut distance = 0.0;
    for [from, to] in path.points.array_windows() {
        let displacement = *to - *from;
        let displacement = Vector3D::new(displacement.x, displacement.y, displacement.z);
        if let Some(sweep) = movement_sweep(
            world_point(*from),
            displacement,
            options.radius,
            segment_height,
        ) {
            append_trigger_crossings(model, sweep, *to, mask, distance, &mut events);
            distance += sweep.distance;
        }
    }
    WalkResult3D {
        position: world_point(movement.position),
        crossings: events.into(),
        collision_contacts: (!movement.contacts.is_empty()).then(|| {
            movement
                .contacts
                .into_iter()
                .map(|(index, hit)| hit.describe(&model.collision_geometry()[index]))
                .collect::<Vec<_>>()
                .into()
        }),
        support: movement.support,
        grounded: movement.grounded,
        hit_wall: movement.hit_wall,
        hit_ceiling: movement.hit_ceiling,
    }
}

#[derive(Default)]
struct MotionPath<const TRACK: bool> {
    enabled: bool,
    points: Vec<Vector3<f64>>,
}

impl<const TRACK: bool> MotionPath<TRACK> {
    fn push(&mut self, point: Vector3<f64>) {
        if TRACK && self.enabled && self.points.last() != Some(&point) {
            self.points.push(point);
        }
    }
}

fn walk<const TRACK: bool, const REPORT: bool, const CONTACTS: bool>(
    model: &SceneModel,
    origin: Point3D,
    displacement: Vector3D,
    segment_height: f64,
    options: &WalkOptions,
    layers: Option<i64>,
    path: &mut MotionPath<TRACK>,
) -> MoveResult {
    let requested = world_vector(displacement);
    let mut original = MoveResult::at(Vector3::new(origin.x, origin.y, origin.z));
    let idle = displacement == Vector3D::default();
    let sweep = if REPORT && idle {
        ShapeSweep::new(origin, Vector3D::new(0.0, -1.0, 0.0), 0.0, options.radius)
            .and_then(|sweep| sweep.with_segment_height(segment_height))
    } else {
        movement_sweep(origin, displacement, options.radius, segment_height)
    };
    let Some(sweep) = sweep else {
        return original;
    };
    let valid_distance = |distance: f64| distance >= 0.0 && (distance as f32).is_finite();
    if !valid_distance(options.step_height)
        || !valid_distance(options.snap_distance)
        || !(0.0..=90.0).contains(&options.max_slope)
    {
        return original;
    }
    let min_normal_y = options.max_slope.to_radians().cos();
    let layers = layers.map_or(MAIN_LAYER, |value| value as u64);
    if layers == 0 {
        return MoveResult::at(sweep.origin + requested);
    }
    path.push(sweep.origin);
    let walk = GroundMotion {
        model,
        sweep,
        layers,
        min_normal_y,
        skin: sweep_skin(sweep.radius),
    };
    let support_distance = 2.0 * walk.skin;
    if REPORT && idle {
        original.set_support::<CONTACTS>(model, walk.land(sweep.origin, support_distance));
        return original;
    }
    let checks_start =
        requested.y <= 0.0 && (options.step_height > 0.0 || options.snap_distance > 0.0);
    let start_support = checks_start
        .then(|| walk.land(sweep.origin, support_distance))
        .flatten();
    let mut ordinary = solve_motion::<TRACK, REPORT, CONTACTS>(
        model,
        sweep,
        requested,
        layers,
        Some(min_normal_y),
        path,
    );

    if start_support.is_some() {
        if ordinary.blocked && options.step_height > 0.0 {
            let mut stepped_path = MotionPath {
                enabled: path.enabled,
                points: Vec::new(),
            };
            if let Some(stepped) = walk.step::<TRACK, REPORT, CONTACTS>(
                requested,
                ordinary.position,
                options,
                &mut stepped_path,
            ) {
                *path = stepped_path;
                return stepped;
            }
        }
        if options.snap_distance > 0.0
            && let Some(landed) = walk.land(ordinary.position, options.snap_distance)
        {
            path.push(landed.position);
            ordinary.accept_landing::<CONTACTS>(model, landed);
            ordinary.grounded = REPORT;
            return ordinary;
        }
    }
    if REPORT {
        let support = if checks_start && ordinary.position == sweep.origin {
            start_support
        } else {
            walk.land(ordinary.position, support_distance)
        };
        ordinary.set_support::<CONTACTS>(model, support);
    }
    ordinary
}

struct Landing {
    position: Vector3<f64>,
    index: usize,
    contact: MeshContact,
}

struct GroundMotion<'a> {
    model: &'a SceneModel,
    sweep: ShapeSweep,
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
    ) -> Option<(usize, MeshContact)> {
        closest_sweep_contact(
            self.model,
            ShapeSweep {
                origin: position,
                direction: Vector3::y() * direction,
                distance,
                blocking_only: true,
                ..self.sweep
            },
            self.layers,
        )
    }

    fn land(&self, position: Vector3<f64>, distance: f64) -> Option<Landing> {
        let (index, contact) = self.vertical_contact(position, -1.0, distance)?;
        if contact.inside || contact.contact.normal.y < self.min_normal_y {
            return None;
        }
        Some(Landing {
            position: position - Vector3::y() * (contact.contact.distance - self.skin).max(0.0),
            index,
            contact,
        })
    }

    fn step<const TRACK: bool, const REPORT: bool, const CONTACTS: bool>(
        &self,
        requested: Vector3<f64>,
        ordinary: Vector3<f64>,
        options: &WalkOptions,
        path: &mut MotionPath<TRACK>,
    ) -> Option<MoveResult> {
        let horizontal = Vector3::new(requested.x, 0.0, requested.z);
        let direction = horizontal.try_normalize(0.0)?;
        let origin = self.sweep.origin;
        let headroom = self.vertical_contact(origin, 1.0, options.step_height);
        let rise = headroom.as_ref().map_or(options.step_height, |(_, hit)| {
            (hit.contact.distance - self.skin).max(0.0)
        });
        if rise <= self.skin {
            return None;
        }
        let raised = origin + rise * Vector3::y();
        path.push(origin);
        path.push(raised);
        let mut crossed = solve_motion::<TRACK, REPORT, CONTACTS>(
            self.model,
            ShapeSweep {
                origin: raised,
                ..self.sweep
            },
            horizontal,
            self.layers,
            Some(self.min_normal_y),
            path,
        );
        if (crossed.position - ordinary).dot(&direction) <= self.skin {
            return None;
        }
        let landed = self.land(crossed.position, rise + options.snap_distance + self.skin)?;
        let height_gain = landed.position.y - origin.y;
        if height_gain > options.step_height + self.skin
            || height_gain < -options.snap_distance - self.skin
        {
            return None;
        }
        path.push(landed.position);
        crossed.grounded = REPORT;
        crossed.hit_ceiling |= REPORT
            && headroom
                .as_ref()
                .is_some_and(|(_, hit)| ceiling_normal(hit.contact.normal.y));
        if CONTACTS {
            for (_, hit) in &mut crossed.contacts {
                hit.contact.distance += rise;
            }
            crossed.traveled += rise;
            if let Some((index, mut hit)) = headroom {
                hit.contact.distance = rise;
                crossed.contacts.insert(0, (index, hit));
            }
        }
        crossed.accept_landing::<CONTACTS>(self.model, landed);
        Some(crossed)
    }
}

fn world_point(value: Vector3<f64>) -> Point3D {
    Point3D::new(value.x, value.y, value.z)
}

fn world_vector(value: Vector3D) -> Vector3<f64> {
    Vector3::new(value.x, value.y, value.z)
}

fn movement_sweep(
    origin: Point3D,
    displacement: Vector3D,
    radius: f64,
    segment_height: f64,
) -> Option<ShapeSweep> {
    let vector = world_vector(displacement);
    let sweep = ShapeSweep::new(origin, displacement, vector.norm(), radius)?
        .with_segment_height(segment_height)?;
    (sweep.origin + vector)
        .iter()
        .all(|&component| (component as f32).is_finite())
        .then_some(sweep)
}

fn sweep_skin(radius: f64) -> f64 {
    radius * 0.0001
}

#[derive(Default)]
struct MoveResult {
    position: Vector3<f64>,
    blocked: bool,
    grounded: bool,
    hit_wall: bool,
    hit_ceiling: bool,
    traveled: f64,
    contacts: Vec<(usize, MeshContact)>,
    support: Hit3D,
}

impl MoveResult {
    fn push_contact(&mut self, index: usize, mut hit: MeshContact) {
        hit.contact.distance = self.traveled;
        self.contacts.push((index, hit));
    }

    fn set_support<const CONTACTS: bool>(&mut self, model: &SceneModel, landing: Option<Landing>) {
        self.grounded = landing.is_some();
        if CONTACTS && let Some(landing) = landing {
            self.support = landing
                .contact
                .describe(&model.collision_geometry()[landing.index]);
        }
    }

    fn accept_landing<const CONTACTS: bool>(&mut self, model: &SceneModel, mut landing: Landing) {
        if CONTACTS {
            let descent = (self.position.y - landing.position.y).max(0.0);
            if descent > 0.0 {
                self.traveled += descent;
                self.push_contact(landing.index, landing.contact.clone());
            }
            landing.contact.contact.distance =
                (landing.contact.contact.distance - descent).max(0.0);
            self.support = landing
                .contact
                .describe(&model.collision_geometry()[landing.index]);
        }
        self.position = landing.position;
    }

    fn at(position: Vector3<f64>) -> Self {
        Self {
            position,
            ..Self::default()
        }
    }
}

// Downward slants also block upward movement. Ignore tiny rounding errors on
// nominally vertical surfaces to keep wall contacts from reporting a ceiling.
fn ceiling_normal(y: f64) -> bool {
    y < -1e-6
}

fn solve_motion<const TRACK: bool, const REPORT: bool, const CONTACTS: bool>(
    model: &SceneModel,
    mut sweep: ShapeSweep,
    mut remaining: Vector3<f64>,
    layers: u64,
    min_normal_y: Option<f64>,
    path: &mut MotionPath<TRACK>,
) -> MoveResult {
    let mut position = sweep.origin;
    let mut result = MoveResult::at(position);
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
        let Some((index, hit)) = closest_sweep_contact(model, sweep, layers) else {
            position += remaining;
            path.push(position);
            if CONTACTS {
                result.traveled += distance;
            }
            break;
        };
        if hit.inside {
            break;
        }
        let advance = (hit.contact.distance - skin).max(0.0);
        let moved = sweep.direction * advance;
        position += moved;
        path.push(position);
        remaining -= moved;

        let normal = hit.contact.normal;
        let mut add_plane = |normal: Vector3<f64>| {
            if planes[..plane_count]
                .iter()
                .any(|plane| plane.dot(&normal) > 1.0 - 1e-10)
            {
                return false;
            }
            planes[plane_count] = normal;
            plane_count += 1;
            true
        };
        let new_constraint = add_plane(normal);
        if CONTACTS {
            result.traveled += advance;
            if new_constraint {
                result.push_contact(index, hit);
            }
        }
        if min_normal_y.is_some_and(|minimum| normal.y < minimum) {
            result.blocked = true;
            if REPORT {
                if ceiling_normal(normal.y) {
                    result.hit_ceiling = true;
                } else {
                    result.hit_wall = true;
                }
            }
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
    result.position = position;
    result
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
    capsule_overlap(model, center, radius, 0.0, layers)
}

/// Return overlapping authored rows for a vertical capsule.
#[doc(hidden)]
pub fn capsule_overlap(
    model: &SceneModel,
    center: Point3D,
    radius: f64,
    segment_height: f64,
    layers: Option<i64>,
) -> VecModel<Hit3D> {
    overlap_shape(model, center, radius, segment_height, layers, false)
}

/// Return the trigger volumes currently touching a vertical capsule.
#[doc(hidden)]
pub fn capsule_trigger_overlap(
    model: &SceneModel,
    center: Point3D,
    radius: f64,
    segment_height: f64,
    layers: Option<i64>,
) -> VecModel<Hit3D> {
    overlap_shape(model, center, radius, segment_height, layers, true)
}

fn overlap_shape(
    model: &SceneModel,
    center: Point3D,
    radius: f64,
    segment_height: f64,
    layers: Option<i64>,
    triggers: bool,
) -> VecModel<Hit3D> {
    let Some(sweep) = ShapeSweep::new(center, Vector3D::new(0.0, 1.0, 0.0), 0.0, radius)
        .and_then(|sweep| sweep.with_segment_height(segment_height))
    else {
        return VecModel::default();
    };
    let layers = layers.map_or(MAIN_LAYER, |value| value as u64);
    if layers == 0 || triggers && !model.has_triggers(layers) {
        return VecModel::default();
    }
    let mut contacts = Vec::new();
    model.visit_sweep(sweep.span(0.0), |index, max| {
        let object = &model.collision_geometry()[index];
        if object.layers & layers != 0 {
            let hit = if triggers {
                object.trigger_contact(sweep, max)
            } else {
                object.sweep_contact(sweep, max)
            };
            if let Some(hit) = hit {
                contacts.push((index, hit));
            }
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
            && model.collision_geometry()[index].scene_row().is_some()
            && model.collision_geometry()[index].scene_row()
                == model.collision_geometry()[*previous_index].scene_row()
        {
            if hit.contact.initial_gap < previous.contact.initial_gap {
                *previous_index = index;
                *previous = hit;
            }
        } else if let Some((previous_index, previous)) = row.replace((index, hit)) {
            results.push(previous.describe(&model.collision_geometry()[previous_index]));
        }
    }
    if let Some((index, hit)) = row {
        results.push(hit.describe(&model.collision_geometry()[index]));
    }
    results.into()
}

/// Report entries and exits along a single linear capsule movement, in travel order.
#[doc(hidden)]
pub fn capsule_trigger_crossings(
    model: &SceneModel,
    from: Point3D,
    to: Point3D,
    radius: f64,
    segment_height: f64,
    layers: Option<i64>,
) -> VecModel<Hit3D> {
    let displacement = Vector3D::new(to.x - from.x, to.y - from.y, to.z - from.z);
    let Some(sweep) = movement_sweep(from, displacement, radius, segment_height) else {
        return VecModel::default();
    };
    let layers = layers.map_or(MAIN_LAYER, |value| value as u64);
    if !model.has_triggers(layers) {
        return VecModel::default();
    }
    let mut events = Vec::new();
    append_trigger_crossings(
        model,
        sweep,
        Vector3::new(to.x, to.y, to.z),
        layers,
        0.0,
        &mut events,
    );
    events.into()
}

// An authored compound trigger is the union of its convex overlap intervals.
// Retain the shape index of each boundary for its contact point and normal.
struct TriggerInterval {
    body: (bool, usize),
    index: usize,
    start: f64,
    end: f64,
    entry: Option<(usize, MeshContact)>,
    exit: Option<(usize, MeshContact)>,
}

impl TriggerInterval {
    fn append(self, contacts: &mut Vec<(usize, MeshContact, bool)>) {
        if let Some((index, hit)) = self.entry {
            contacts.push((index, hit, true));
        }
        if let Some((index, hit)) = self.exit {
            contacts.push((index, hit, false));
        }
    }
}

fn merge_trigger_intervals(
    mut intervals: Vec<TriggerInterval>,
    tolerance: f64,
    contacts: &mut Vec<(usize, MeshContact, bool)>,
) {
    intervals.sort_unstable_by(|left, right| {
        left.body
            .cmp(&right.body)
            .then(left.start.total_cmp(&right.start))
            .then(left.index.cmp(&right.index))
    });
    let mut pending: Option<TriggerInterval> = None;
    for interval in intervals {
        if let Some(current) = pending.as_mut()
            && current.body == interval.body
            && interval.start <= current.end + tolerance
        {
            if interval.start == current.start && interval.entry.is_none() {
                current.entry = None;
            }
            if interval.end > current.end
                || (interval.end == current.end && interval.exit.is_none())
            {
                current.end = interval.end;
                current.exit = interval.exit;
            }
        } else if let Some(previous) = pending.replace(interval) {
            previous.append(contacts);
        }
    }
    if let Some(interval) = pending {
        interval.append(contacts);
    }
}

fn append_trigger_crossings(
    model: &SceneModel,
    forward: ShapeSweep,
    end: Vector3<f64>,
    layers: u64,
    traveled: f64,
    events: &mut Vec<Hit3D>,
) {
    let reverse = ShapeSweep {
        origin: end,
        direction: -forward.direction,
        ..forward
    };
    let at_start = |sweep: ShapeSweep| ShapeSweep {
        distance: 0.0,
        ..sweep
    };
    let mut contacts = Vec::new();
    let mut intervals = model.has_collision_shapes().then(Vec::new);
    let tolerance = (forward.distance + forward.radius) * 1e-10;
    model.visit_sweep(forward.span(forward.distance), |index, max| {
        let object = &model.collision_geometry()[index];
        if !object.is_trigger() || object.layers & layers == 0 {
            return max;
        }
        let starts_inside = object.trigger_contact(at_start(forward), 0.0).is_some();
        let ends_inside = object.trigger_contact(at_start(reverse), 0.0).is_some();
        let entry = if starts_inside {
            None
        } else if let Some(hit) = object.trigger_contact(forward, forward.distance) {
            Some(hit)
        } else {
            return max;
        };
        let exit = if ends_inside {
            None
        } else if let Some(mut hit) = object.trigger_contact(reverse, forward.distance) {
            hit.contact.distance = (forward.distance - hit.contact.distance).max(0.0);
            Some(hit)
        } else {
            return max;
        };
        // A convex trigger has one overlap interval. Isolated tangencies
        // strictly inside the segment do not generate a pair of crossings.
        if let (Some(entry), Some(exit)) = (&entry, &exit)
            && exit.contact.distance - entry.contact.distance <= tolerance
        {
            return max;
        }
        if let Some(intervals) = &mut intervals {
            intervals.push(TriggerInterval {
                body: object
                    .scene_row()
                    .map_or((true, index), |row| (false, row as usize)),
                index,
                start: entry.as_ref().map_or(0.0, |hit| hit.contact.distance),
                end: exit
                    .as_ref()
                    .map_or(forward.distance, |hit| hit.contact.distance),
                entry: entry.map(|hit| (index, hit)),
                exit: exit.map(|hit| (index, hit)),
            });
        } else {
            if let Some(hit) = entry {
                contacts.push((index, hit, true));
            }
            if let Some(hit) = exit {
                contacts.push((index, hit, false));
            }
        }
        max
    });
    if let Some(intervals) = intervals {
        merge_trigger_intervals(intervals, tolerance, &mut contacts);
    }
    contacts.sort_unstable_by(|(a, first, entering_a), (b, second, entering_b)| {
        first
            .contact
            .distance
            .total_cmp(&second.contact.distance)
            .then(a.cmp(b))
            .then(entering_b.cmp(entering_a))
    });
    events.extend(contacts.into_iter().map(|(index, mut hit, entering)| {
        hit.contact.distance += traveled;
        hit.describe_contact(&model.collision_geometry()[index], entering)
    }));
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

pub(crate) fn cast_shape_triangle(
    sweep: ShapeSweep,
    points: [Vector3<f64>; 3],
    inside: bool,
    max: f64,
) -> Option<(TriangleContact, bool)> {
    if sweep.half_segment == 0.0 {
        return cast_triangle(sweep, points, inside, max).map(|contact| (contact, inside));
    }

    // The capsule intersects a triangle when its center sphere intersects
    // that triangle extruded by the capsule segment. Test the prism's caps
    // and sides, then map the winning barycentrics back to the source triangle.
    let triangle = Triangle::new(points);
    if sweep.blocking_only
        && !inside
        && let Some(normal) = triangle.normal
    {
        let plane = (sweep.origin - points[0]).dot(&normal);
        let support = sweep.radius + sweep.half_segment * normal.y.abs();
        // A capsule on the outside of the infinite plane can slide along it.
        // Skip prism edges here to avoid numerical blocking on triangle seams.
        if plane.abs() >= support * (1.0 - 1e-12)
            && plane.signum() * sweep.direction.dot(&normal) >= 0.0
        {
            return None;
        }
    }
    if let Some(normal) = triangle.normal
        && normal.y != 0.0
    {
        let height = (points[0] - sweep.origin).dot(&normal) / normal.y;
        if height.abs() <= sweep.half_segment {
            let location = sweep.origin + Vector3::y() * height;
            if let Some(weights) = triangle.weights(location, normal) {
                return Some((
                    TriangleContact {
                        distance: 0.0,
                        location,
                        normal: if normal.dot(&sweep.direction) > 0.0 {
                            -normal
                        } else {
                            normal
                        },
                        weights,
                        initial_gap: 0.0,
                    },
                    true,
                ));
            }
        }
    }

    let offset = Vector3::y() * sweep.half_segment;
    let low = points.map(|point| point - offset);
    let high = points.map(|point| point + offset);
    let mut best: Option<TriangleContact> = None;
    let mut inspect = |face, vertices: [usize; 3]| {
        let limit = best.as_ref().map_or(max, |hit| hit.distance);
        let Some(mut hit) = cast_triangle(sweep, face, inside, limit) else {
            return;
        };
        if best.as_ref().is_some_and(|old| {
            hit.distance
                .total_cmp(&old.distance)
                .then(hit.initial_gap.total_cmp(&old.initial_gap))
                .is_ge()
        }) {
            return;
        }
        let mut weights = [0.0; 3];
        for (index, weight) in vertices.into_iter().zip(hit.weights) {
            weights[index] += weight;
        }
        hit.weights = weights;
        hit.location = points[0] * weights[0] + points[1] * weights[1] + points[2] * weights[2];
        best = Some(hit);
    };
    inspect(low, [0, 1, 2]);
    inspect(high, [0, 1, 2]);
    for a in 0..3 {
        let b = (a + 1) % 3;
        inspect([low[a], low[b], high[b]], [a, b, b]);
        inspect([low[a], high[b], high[a]], [a, b, a]);
    }
    best.map(|contact| (contact, inside))
}

fn cast_triangle(
    sweep: ShapeSweep,
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

    use crate::{Color, RecordList, RecordValue, Value, scene3d::record_objects};

    fn box_body(id: &str, position: [f64; 3], scale: [f64; 3], trigger: bool) -> RecordValue {
        let [x, y, z] = position;
        let [sx, sy, sz] = scale;
        RecordValue::new(
            "Body",
            [
                ("id", Value::String(id.into())),
                ("mesh", Value::String("primitive:box".into())),
                ("x", Value::Float(x)),
                ("y", Value::Float(y)),
                ("z", Value::Float(z)),
                ("scale-x", Value::Float(sx)),
                ("scale-y", Value::Float(sy)),
                ("scale-z", Value::Float(sz)),
                ("yaw", Value::Float(0.0)),
                ("pitch", Value::Float(0.0)),
                ("roll", Value::Float(0.0)),
                ("color", Value::Color(Color(0xffffffff))),
                ("trigger", Value::Bool(trigger)),
            ]
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect(),
        )
    }

    fn compound_body(body: RecordValue, parts: &[Point3D]) -> RecordValue {
        let mut fields: std::collections::BTreeMap<_, _> = body
            .fields()
            .map(|(name, value)| (name.to_owned(), value.clone()))
            .collect();
        let shapes = parts
            .iter()
            .map(|position| {
                RecordValue::new(
                    "Shape",
                    [
                        ("shape".into(), Value::String("box".into())),
                        ("position".into(), Value::Point3D(*position)),
                    ]
                    .into_iter()
                    .collect(),
                )
            })
            .collect();
        fields.insert(
            "collision-shapes".into(),
            Value::RecordList(RecordList::new("Shape", shapes)),
        );
        RecordValue::new(body.name(), fields)
    }

    #[test]
    fn compound_shapes_follow_parent_pose_and_keep_render_picking_separate() {
        let parent = box_body("parent", [4.0, 0.0, 0.0], [1.0; 3], false)
            .with_field(&["mesh".into()], Value::String(String::new()))
            .expect("mesh field")
            .with_field(&["yaw".into()], Value::Float(90.0))
            .expect("yaw field");
        let child = compound_body(
            box_body("child", [0.0; 3], [1.0; 3], false),
            &[Point3D::new(0.0, 0.0, -2.0)],
        );
        let mut fields: std::collections::BTreeMap<_, _> = child
            .fields()
            .map(|(name, value)| (name.to_owned(), value.clone()))
            .collect();
        fields.insert("parent".into(), Value::String("parent".into()));
        let scene = RecordList::new("Body", vec![parent, RecordValue::new("Body", fields)]);
        let model = record_objects(&scene, |_| None, |_| None);
        assert_eq!(model.objects().len(), 1);
        assert_eq!(model.collision_geometry().len(), 1);
        let origin = Point3D::default();
        let dir = Vector3D::new(1.0, 0.0, 0.0);
        let visual = crate::scene3d::ray_hit(&model, origin, dir, 6.0, None);
        let physical = sphere_cast(&model, origin, dir, 6.0, 0.25, None);
        assert_eq!(visual.object_id, "child");
        assert_eq!(physical.object_id, "child");
        assert_eq!(physical.row_index, 1);
        assert!((visual.distance - 3.5).abs() < 1e-6);
        assert!((physical.position.x - 1.5).abs() < 1e-6);
        assert!((physical.distance - 1.25).abs() < 1e-6);
    }

    #[test]
    fn compound_trigger_merges_overlapping_parts_but_keeps_separate_passages() {
        let body = compound_body(
            box_body("sensor", [0.0; 3], [1.0; 3], true)
                .with_field(&["mesh".into()], Value::String(String::new()))
                .expect("mesh field"),
            &[
                Point3D::new(-0.4, 0.0, 0.0),
                Point3D::new(0.4, 0.0, 0.0),
                Point3D::new(2.0, 0.0, 0.0),
            ],
        );
        let rows = RecordList::new("Body", vec![body]);
        let model = record_objects(&rows, |_| None, |_| None);
        assert!(model.objects().is_empty());
        let overlaps = capsule_trigger_overlap(&model, Point3D::default(), 0.2, 0.0, None);
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps.row(0).expect("sensor contact").row_index, 0);
        let end = Point3D::new(4.0, 0.0, 0.0);
        let crossings =
            capsule_trigger_crossings(&model, Point3D::new(-2.0, 0.0, 0.0), end, 0.2, 0.0, None)
                .snapshot();
        assert_eq!(crossings.len(), 4);
        for ((hit, expected), entering) in crossings
            .iter()
            .zip([0.9, 3.1, 3.3, 4.7])
            .zip([true, false, true, false])
        {
            assert_eq!(
                (hit.object_id.as_str(), hit.row_index, hit.entering),
                ("sensor", 0, entering)
            );
            assert!((hit.distance - expected).abs() < 1e-6, "{hit:?}");
        }
        let inside = capsule_trigger_crossings(&model, Point3D::default(), end, 0.2, 0.0, None)
            .row(0)
            .expect("outer exit");
        assert!(!inside.entering && (inside.distance - 1.1).abs() < 1e-6);
        assert!(
            !sphere_cast(
                &model,
                Point3D::new(-2.0, 0.0, 0.0),
                Vector3D::new(1.0, 0.0, 0.0),
                6.0,
                0.2,
                None
            )
            .valid
        );
    }

    fn collider_body(
        id: &str,
        shape: &str,
        position: Point3D,
        size: Vector3D,
        mesh: &str,
        trigger: bool,
    ) -> RecordValue {
        let body = compound_body(box_body(id, [0.0; 3], [1.0; 3], trigger), &[])
            .with_field(&["mesh".into()], Value::String(String::new()))
            .expect("mesh field");
        let collider = RecordValue::new(
            "Shape",
            [
                ("shape".into(), Value::String(shape.into())),
                ("position".into(), Value::Point3D(position)),
                ("size".into(), Value::Vector3D(size)),
                ("mesh".into(), Value::String(mesh.into())),
            ]
            .into_iter()
            .collect(),
        );
        body.with_field(
            &["collision-shapes".into()],
            Value::RecordList(RecordList::new("Shape", vec![collider])),
        )
        .expect("collider field")
    }

    #[test]
    fn capsule_colliders_report_solid_interiors_and_trigger_crossings() {
        let from = Point3D::new(-2.0, 0.0, 0.0);
        let to = Point3D::new(2.0, 0.0, 0.0);
        let direction = Vector3D::new(1.0, 0.0, 0.0);
        for trigger in [false, true] {
            let body = collider_body(
                "capsule",
                "capsule",
                Point3D::default(),
                Vector3D::new(1.0, 2.4, 1.0),
                "",
                trigger,
            );
            let model = record_objects(&RecordList::new("Body", vec![body]), |_| None, |_| None);
            assert!(model.objects().is_empty());
            if trigger {
                let overlaps = capsule_trigger_overlap(&model, Point3D::default(), 0.05, 0.0, None);
                assert_eq!(overlaps.len(), 1);
                let crossings =
                    capsule_trigger_crossings(&model, from, to, 0.15, 0.0, None).snapshot();
                assert_eq!(crossings.len(), 2);
                assert!(crossings[0].entering && !crossings[1].entering);
                assert!((crossings[0].distance - 1.35).abs() < 1e-6);
                assert!((crossings[1].distance - 2.65).abs() < 1e-6);
                assert!(!sphere_cast(&model, from, direction, 4.0, 0.15, None).valid);
            } else {
                let inside = sphere_cast(&model, Point3D::default(), direction, 2.0, 0.05, None);
                assert!(inside.valid, "{inside:?}");
                assert_eq!(inside.distance, 0.0);
                let hit = sphere_cast(&model, from, direction, 4.0, 0.15, None);
                assert!(hit.valid && (hit.distance - 1.35).abs() < 1e-6, "{hit:?}");
            }
        }
    }

    #[test]
    fn mesh_collider_keeps_imported_rest_transforms_and_source_identity() {
        use crate::scene3d::Mesh3D;
        use crate::scene3d::mesh::animation::{
            AnimatedMesh, MeshAnimationNode, MeshAsset, MeshNodeTransform,
        };
        use std::sync::Arc;

        let mesh = Mesh3D::prepare_procedural(vec![("surface", Mesh3D::Box.vertices().to_vec())])
            .expect("triangle mesh");
        let node = |source_node, parent, mesh, x| MeshAnimationNode {
            source_node,
            parent,
            mesh,
            skin: None,
            morph_weights: Default::default(),
            transform: MeshNodeTransform::Trs {
                translation: [x, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
            instances: None,
        };
        let asset = MeshAsset::Animated(Arc::new(
            AnimatedMesh::new(
                vec![mesh],
                vec![node(8, None, None, 1.0), node(22, Some(0), Some(0), 2.0)],
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .expect("asset nodes"),
        ));
        let body = collider_body(
            "wall",
            "mesh",
            Point3D::new(1.0, 0.0, 0.0),
            Vector3D::new(1.0, 1.0, 1.0),
            "asset:rest.gltf",
            false,
        )
        .with_field(&["x".into()], Value::Float(4.0))
        .expect("owner position");
        let model = record_objects(
            &RecordList::new("Body", vec![body.clone()]),
            |key| (key == "asset:rest.gltf").then(|| asset.clone()),
            |_| None,
        );
        assert!(model.objects().is_empty());
        let dir = Vector3D::new(1.0, 0.0, 0.0);
        let hit = sphere_cast(&model, Point3D::default(), dir, 10.0, 0.2, None);
        assert_eq!(
            (hit.object_id.as_str(), hit.row_index, hit.mesh_node_index),
            ("wall", 0, 22)
        );
        assert!((hit.distance - 7.3).abs() < 1e-6, "{hit:?}");
        assert!((hit.position.x - 7.5).abs() < 1e-6);

        let trigger = body
            .with_field(&["trigger".into()], Value::Bool(true))
            .expect("trigger field");
        let model = record_objects(
            &RecordList::new("Body", vec![trigger]),
            |_| Some(asset.clone()),
            |_| None,
        );
        assert!(!sphere_cast(&model, Point3D::default(), dir, 10.0, 0.2, None).valid);
        assert!(
            capsule_trigger_crossings(
                &model,
                Point3D::default(),
                Point3D::new(10.0, 0.0, 0.0),
                0.2,
                0.0,
                None
            )
            .is_empty()
        );
    }

    #[test]
    fn one_sweep_interleaves_entries_and_exits_in_travel_order() {
        let rows = RecordList::new(
            "Body",
            vec![
                box_body("later", [1.2, 0.0, 0.0], [0.4; 3], true),
                box_body("earlier", [-1.2, 0.0, 0.0], [0.4; 3], true),
            ],
        );
        let model = record_objects(&rows, |_| None, |_| None);
        let crossings = capsule_trigger_crossings(
            &model,
            Point3D::new(-2.0, 0.0, 0.0),
            Point3D::new(2.0, 0.0, 0.0),
            0.2,
            0.8,
            None,
        )
        .snapshot();
        let events: Vec<_> = crossings
            .iter()
            .map(|hit| (hit.object_id.as_str(), hit.entering))
            .collect();
        assert_eq!(
            events,
            [
                ("earlier", true),
                ("earlier", false),
                ("later", true),
                ("later", false)
            ]
        );
        for (hit, expected) in crossings.iter().zip([0.4, 1.2, 2.8, 3.6]) {
            assert!((hit.distance - expected).abs() < 1e-6, "{hit:?}");
        }
    }

    #[test]
    fn step_triggers_follow_the_committed_route_and_ignore_the_straight_chord() {
        let rows = RecordList::new(
            "Body",
            vec![
                box_body("floor", [0.0, -0.13, 0.0], [9.0, 0.26, 7.4], false),
                box_body("platform", [-0.55, 0.12, 0.0], [0.8, 0.24, 1.85], false),
                box_body("overhead", [-2.4, 1.03, 0.0], [0.08; 3], true),
            ],
        );
        let model = record_objects(&rows, |_| None, |_| None);
        let origin = Point3D::new(-2.4, 0.4, 0.0);
        let displacement = Vector3D::new(1.8, 0.0, 0.0);
        let options = || WalkOptions {
            radius: 0.4,
            step_height: 0.3,
            snap_distance: 0.36,
            max_slope: 45.0,
        };
        let end = capsule_walk(&model, origin, displacement, 0.0, options(), None);
        assert!(end.y > 0.64 && end.x > -0.61, "{end:?}");
        assert!(capsule_trigger_crossings(&model, origin, end, 0.4, 0.0, None).is_empty());
        let movement =
            capsule_walk_with_contacts(&model, origin, displacement, 0.0, options(), None);
        assert_eq!(movement.position, end);
        assert!(movement.grounded);
        assert!(!movement.hit_wall && !movement.hit_ceiling);
        assert!(movement.support.valid);
        assert_eq!(movement.support.object_id, "platform");
        assert!((movement.support.position.y - 0.24).abs() < 1e-6);
        assert!(movement.support.distance <= 0.00005);
        let contacts = movement.contacts().snapshot();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].object_id, "platform");
        assert!(contacts[0].normal.y > 0.999);
        assert!(contacts[0].distance > 2.1);
        let crossings = movement.crossings.snapshot();
        assert_eq!(crossings.len(), 2);
        assert_eq!(crossings[0].object_id, "overhead");
        assert_eq!(crossings[1].object_id, "overhead");
        assert!(crossings[0].entering && !crossings[1].entering);
        assert!((crossings[0].distance - 0.19).abs() < 1e-6);
        assert!(crossings[1].distance > 0.3);
        assert!(
            capsule_walk_result(
                &model,
                origin,
                displacement,
                0.0,
                WalkOptions {
                    step_height: 0.0,
                    ..options()
                },
                None,
            )
            .crossings
            .is_empty()
        );
        let masked = capsule_walk_result(&model, origin, displacement, 0.0, options(), Some(0));
        assert_eq!(
            masked.position,
            Point3D::new(origin.x + displacement.x, origin.y, origin.z)
        );
        assert!(masked.crossings.is_empty());
        let stationary =
            capsule_walk_result(&model, origin, Vector3D::default(), 0.0, options(), None);
        assert_eq!(stationary.position, origin);
        assert!(stationary.crossings.is_empty());
        assert!(stationary.grounded);
    }

    #[test]
    fn walk_contacts_report_final_support_and_blocking_surfaces() {
        let rows = RecordList::new(
            "Body",
            vec![
                box_body("floor", [0.0, -0.25, 0.0], [8.0, 0.5, 8.0], false),
                box_body("wall", [1.0, 0.75, 0.0], [0.25, 1.5, 2.0], false),
                box_body("roof", [0.0, 1.5, 0.0], [1.0, 0.25, 2.0], false),
            ],
        );
        let model = record_objects(&rows, |_| None, |_| None);
        let start = Point3D::new(0.0, 0.55, 0.0);
        let advance = |from, delta, layers| {
            capsule_walk_with_contacts(
                &model,
                from,
                delta,
                0.6,
                WalkOptions {
                    radius: 0.25,
                    step_height: 0.0,
                    snap_distance: 0.0,
                    max_slope: 45.0,
                },
                layers,
            )
        };
        let resting = advance(start, Vector3D::default(), None);
        assert!(resting.grounded && !resting.hit_wall && !resting.hit_ceiling);
        assert_eq!(resting.position, start);
        assert!(resting.contacts().is_empty());
        assert!(resting.support.valid);
        assert_eq!(resting.support.object_id, "floor");
        assert_eq!(resting.support.row_index, 0);
        assert!(resting.support.normal.y > 0.999);
        assert!(resting.support.distance < 1e-8);
        let hit_wall = advance(start, Vector3D::new(2.0, 0.0, 0.0), None);
        assert!(hit_wall.grounded && hit_wall.hit_wall && !hit_wall.hit_ceiling);
        assert!((0.60..0.64).contains(&hit_wall.position.x), "{hit_wall:?}");
        let wall_hits = hit_wall.contacts().snapshot();
        assert_eq!(wall_hits.len(), 1);
        let wall = &wall_hits[0];
        assert_eq!(wall.object_id, "wall");
        assert_eq!(wall.row_index, 1);
        assert!((wall.position.x - 0.875).abs() < 1e-8 && wall.normal.x < -0.999);
        assert!((wall.distance - hit_wall.position.x).abs() < 1e-8);
        let light = capsule_walk_result(
            &model,
            start,
            Vector3D::new(2.0, 0.0, 0.0),
            0.6,
            WalkOptions {
                radius: 0.25,
                step_height: 0.0,
                snap_distance: 0.0,
                max_slope: 45.0,
            },
            None,
        );
        assert_eq!(light.position, hit_wall.position);
        assert!(light.hit_wall && light.collision_contacts.is_none());
        assert!(light.grounded && !light.support.valid);
        let hit_roof = advance(start, Vector3D::new(0.0, 1.0, 0.0), None);
        assert!(!hit_roof.grounded && !hit_roof.hit_wall && hit_roof.hit_ceiling);
        assert!(!hit_roof.support.valid);
        assert!((0.80..0.84).contains(&hit_roof.position.y), "{hit_roof:?}");
        let roof = hit_roof.contacts().row(0).expect("roof contact");
        assert_eq!(roof.object_id, "roof");
        assert!(roof.normal.y < -0.999);
        let landed = advance(hit_roof.position, Vector3D::new(0.0, -1.0, 0.0), None);
        assert!(landed.grounded && !landed.hit_wall && !landed.hit_ceiling);
        let floor = landed.contacts().row(0).expect("landing contact");
        assert_eq!(floor.object_id, "floor");
        assert!(floor.normal.y > 0.999);
        let off_edge = advance(start, Vector3D::new(-6.0, 0.0, 0.0), None);
        assert!(!off_edge.grounded && !off_edge.hit_wall && !off_edge.hit_ceiling);
        assert!(!off_edge.support.valid);
        let masked = advance(start, Vector3D::default(), Some(0));
        assert!(!masked.grounded && !masked.hit_wall && !masked.hit_ceiling);
    }

    #[test]
    fn walk_reports_blocking_contacts_in_slide_order() {
        let rows = RecordList::new(
            "Body",
            vec![
                box_body("second", [0.0, 0.5, 1.75], [8.0, 2.0, 0.25], false),
                box_body("first", [1.0, 0.5, 0.0], [0.25, 2.0, 8.0], false),
            ],
        );
        let model = record_objects(&rows, |_| None, |_| None);
        let result = capsule_walk_with_contacts(
            &model,
            Point3D::new(0.0, 0.55, 0.0),
            Vector3D::new(1.5, 0.0, 3.0),
            0.6,
            WalkOptions {
                radius: 0.25,
                step_height: 0.0,
                snap_distance: 0.0,
                max_slope: 45.0,
            },
            None,
        );
        let contacts = result.contacts().snapshot();
        assert_eq!(contacts.len(), 2);
        assert_eq!(contacts[0].object_id, "first");
        assert_eq!(contacts[1].object_id, "second");
        assert!(contacts[0].distance < contacts[1].distance);
        assert!(contacts[0].normal.x < -0.999 && contacts[1].normal.z < -0.999);
    }

    fn sweep(start: [f64; 3], direction: [f64; 3], radius: f64) -> ShapeSweep {
        let [x, y, z] = start;
        let [dx, dy, dz] = direction;
        ShapeSweep::new(
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
    fn capsule_shaft_hits_a_thin_surface_between_its_end_caps() {
        let beam = [
            Vector3::new(-0.5, 0.43, 0.0),
            Vector3::new(0.5, 0.43, 0.0),
            Vector3::new(0.0, 0.52, 0.0),
        ];
        let sphere = sweep([0.0, 0.0, 2.0], [0.0, 0.0, -1.0], 0.1);
        for height in [-1.0, 0.0, 1.0] {
            let mut cap = sphere;
            cap.origin.y = height;
            assert!(cast_shape_triangle(cap, beam, false, 10.0).is_none());
        }
        let capsule = sphere.with_segment_height(2.0).expect("capsule");
        let (hit, inside) = cast_shape_triangle(capsule, beam, false, 10.0).expect("shaft contact");
        assert!(!inside);
        assert!((hit.distance - 1.9).abs() < 1e-10, "{hit:?}");
        assert!(hit.normal.z > 0.9999);
        assert_eq!(hit.location.z, 0.0);
        assert!((0.43..=0.52).contains(&hit.location.y));
        assert!((hit.weights.into_iter().sum::<f64>() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn capsule_spine_reports_initial_intersection_and_floor_allows_sliding() {
        let floor = [
            Vector3::new(-3.0, 0.0, -3.0),
            Vector3::new(3.0, 0.0, -3.0),
            Vector3::new(0.0, 0.0, 3.0),
        ];
        let capsule = sweep([0.0, 0.4, 0.0], [1.0, 0.0, 0.0], 0.1)
            .with_segment_height(2.0)
            .expect("capsule");
        let (hit, inside) =
            cast_shape_triangle(capsule, floor, false, 10.0).expect("spine intersection");
        assert!(inside);
        assert_eq!(hit.distance, 0.0);
        assert_eq!(hit.location, Vector3::zeros());
        let grounded = ShapeSweep {
            origin: Vector3::new(0.0, 1.1, 0.0),
            blocking_only: true,
            ..capsule
        };
        let contact = cast_shape_triangle(grounded, floor, false, 10.0);
        assert!(contact.is_none(), "{contact:?}");
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
    use crate::{Color, scene3d::visibility::VisibilityIndex};

    #[test]
    fn sphere_inside_a_builtin_solid_gets_an_exit_contact() {
        let sweep = ShapeSweep::new(Point3D::default(), Vector3D::new(1.0, 0.0, 0.0), 3.0, 0.05)
            .expect("test sweep");
        let box_object =
            SceneObject::cuboid("box", [0.0; 3], [1.0; 3], [0.0; 3], Color(0xffffffff))
                .expect("box");
        let sphere = SceneObject::sphere("ball", [0.0; 3], 1.0, Color(0xffffffff)).expect("sphere");
        for object in [box_object, sphere] {
            let hit = object
                .sweep_contact(sweep, 3.0)
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
        let sweep = ShapeSweep::new(
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
            if let Some(hit) = objects[index].sweep_contact(sweep, max) {
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
