//! Resolve look-at dependencies before publishing world transforms.

use std::collections::HashMap;

use nalgebra::{UnitQuaternion, Vector3};

use super::{Parent, Pose, SceneNode, SceneSocket, WorldPose};
use crate::{Point3D, Vector3D, scene3d::rotation::look_at_basis};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct NodeAim {
    target: String,
    up: Option<Vector3D>,
}

impl NodeAim {
    pub(super) fn new(target: &str, up: Option<Vector3D>) -> Option<Box<Self>> {
        (!target.is_empty()).then(|| {
            Box::new(Self {
                target: target.into(),
                up,
            })
        })
    }

    fn pose(
        &self,
        world: WorldPose,
        anchor: Vector3<f32>,
        target: Vector3<f32>,
    ) -> Option<WorldPose> {
        let tolerance = 4.0 * f32::EPSILON * anchor.amax().max(target.amax()).max(1.0);
        if (target - anchor).amax() <= tolerance {
            return None;
        }
        let transform = world.transform;
        let local_up = transform.linear.column(1);
        let up = |[x, y, z]: [f32; 3]| Vector3D::new(x.into(), y.into(), z.into());
        let point = |[x, y, z]: [f32; 3]| Point3D::new(x.into(), y.into(), z.into());
        let original_up = up(local_up.into());
        let basis = look_at_basis(
            Point3D::default(),
            point((-transform.linear.column(2)).into()),
            original_up,
            Some(transform.linear.column(0).into_owned().cast::<f64>()),
        )?;
        let target_basis = look_at_basis(
            point(anchor.into()),
            point(target.into()),
            self.up
                .filter(|up| *up != Vector3D::default())
                .unwrap_or(original_up),
            Some(basis * Vector3::x()),
        )?;
        // Apply a rigid delta in world space. Even a sheared or mirrored
        // parent keeps its inherited distances, and mesh -Z follows the aim.
        let delta = (target_basis * basis.inverse()).cast::<f32>();
        let transform = super::SceneTransform {
            linear: delta * transform.linear,
            origin: anchor + delta * (transform.origin - anchor),
            inverse: transform.inverse * delta.inverse(),
            reflected: transform.reflected,
        }
        .checked()?;
        Some(WorldPose {
            transform,
            rotation: Some(UnitQuaternion::from_rotation_matrix(&target_basis.cast())),
            aimed: true,
            ..world
        })
    }
}

pub(super) fn resolve(
    nodes: &[SceneNode],
    parents: &[Parent],
    sockets: &HashMap<usize, Option<SceneSocket>>,
    ids: &HashMap<&str, Option<usize>>,
    poses: &mut [Pose],
) {
    let parent_index = |index: usize| match parents[index] {
        Parent::Node(parent) if !nodes[index].world_locked => Some(parent),
        _ => None,
    };
    let mut targets: Vec<_> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let aim = node.aim.as_ref().filter(|_| !node.world_locked)?;
            let target = ids.get(aim.target.as_str()).copied().flatten()?;
            (target != index
                && matches!(poses[index], Pose::Ready(_))
                && matches!(poses[target], Pose::Ready(world) if world.visible))
            .then_some(target)
        })
        .collect();
    if targets.iter().all(Option::is_none) {
        return;
    }

    // A row's anchor depends on its parent's full pose and its socket, but
    // not on its own aim. Two independent rows can therefore face each other.
    let mut dependencies: Vec<_> = targets
        .iter()
        .enumerate()
        .map(|(index, target)| {
            if !matches!(poses[index], Pose::Ready(_)) {
                return [None; 2];
            }
            [parent_index(index), target.and_then(parent_index)]
        })
        .collect();
    let (mut order, has_cycle) = dependency_order(&dependencies);
    if has_cycle {
        let components = strongly_connected(&dependencies, &order);
        for (index, edges) in dependencies.iter_mut().enumerate() {
            if edges[1].is_some_and(|dependency| components[index] == components[dependency]) {
                edges[1] = None;
                targets[index] = None;
            }
        }
        order = dependency_order(&dependencies).0;
    }

    let mut changed = vec![false; nodes.len()];
    for index in order {
        let Pose::Ready(previous) = poses[index] else {
            continue;
        };
        let target = targets[index];
        if target.is_none() && !parent_index(index).is_some_and(|parent| changed[parent]) {
            continue;
        }
        let node = &nodes[index];
        let inherited = |row| {
            let parent = parent_index(row)?;
            let Pose::Ready(world) = poses[parent] else {
                return None;
            };
            world.with_socket(sockets.get(&row))
        };
        let anchor = |row: usize| {
            let node = &nodes[row];
            if node.world_locked || parent_index(row).is_none() {
                return Some(node.position);
            }
            let parent = inherited(row)?.transform;
            Some(parent.linear * node.position + parent.origin)
        };
        let track = previous.rotation.is_some();
        let world = if parent_index(index).is_none() {
            Some(previous)
        } else {
            inherited(index).and_then(|parent| parent.child(node, track))
        };
        poses[index] = world.map_or(Pose::Unavailable, |world| {
            let aimed = node
                .aim
                .as_ref()
                .zip(anchor(index))
                .zip(target.and_then(anchor))
                .and_then(|((aim, anchor), target)| aim.pose(world, anchor, target));
            Pose::Ready(aimed.unwrap_or(world))
        });
        changed[index] = true;
    }
}

fn dependency_order(dependencies: &[[Option<usize>; 2]]) -> (Vec<usize>, bool) {
    #[derive(Clone, Copy, PartialEq)]
    enum Visit {
        New,
        Entered,
        Done,
    }
    let mut status = vec![Visit::New; dependencies.len()];
    let mut order = Vec::with_capacity(dependencies.len());
    let mut stack = Vec::new();
    let mut cycle = false;
    for root in 0..dependencies.len() {
        if status[root] != Visit::New {
            continue;
        }
        status[root] = Visit::Entered;
        stack.push((root, 0));
        while let Some((node, next)) = stack.last_mut() {
            let Some(edge) = dependencies[*node].get(*next) else {
                status[*node] = Visit::Done;
                order.push(*node);
                stack.pop();
                continue;
            };
            *next += 1;
            if let Some(edge) = *edge {
                match status[edge] {
                    Visit::New => {
                        status[edge] = Visit::Entered;
                        stack.push((edge, 0));
                    }
                    Visit::Entered => cycle = true,
                    Visit::Done => {}
                }
            }
        }
    }
    (order, cycle)
}

// The second pass of Kosaraju's algorithm walks reverse edges in decreasing finish order.
fn strongly_connected(dependencies: &[[Option<usize>; 2]], order: &[usize]) -> Vec<usize> {
    let size = dependencies.len();
    let mut incoming = vec![None; size];
    let mut edges = Vec::with_capacity(2 * size);
    for (node, dependencies) in dependencies.iter().enumerate() {
        for &dependency in dependencies.iter().flatten() {
            edges.push((node, incoming[dependency].replace(edges.len())));
        }
    }
    let mut components = vec![size; size];
    let mut stack = Vec::new();
    for &root in order.iter().rev() {
        if components[root] != size {
            continue;
        }
        components[root] = root;
        stack.push(root);
        while let Some(node) = stack.pop() {
            let mut edge = incoming[node];
            while let Some(index) = edge {
                let (child, next) = edges[index];
                edge = next;
                if components[child] == size {
                    components[child] = root;
                    stack.push(child);
                }
            }
        }
    }
    components
}

#[cfg(test)]
mod tests {
    use super::{dependency_order, strongly_connected};

    #[test]
    fn cycle_members_exclude_nodes_that_only_depend_on_the_cycle() {
        let edges = [
            [None, Some(2)],
            [None, Some(0)],
            [Some(1), None],
            [Some(2), None],
            [None, None],
            [None, Some(4)],
        ];
        let (order, cyclic) = dependency_order(&edges);
        assert!(cyclic);
        let groups = strongly_connected(&edges, &order);
        assert_eq!(groups[0], groups[1]);
        assert_eq!(groups[1], groups[2]);
        assert_ne!(groups[2], groups[3]);
        assert_ne!(groups[4], groups[5]);
    }
}
