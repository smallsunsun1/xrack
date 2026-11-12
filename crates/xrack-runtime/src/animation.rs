use crate::{Brush, Color, GradientStop, Length, Node, Percent, Value};
use std::{borrow::Cow, collections::BTreeMap, sync::Arc, time::Duration};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Easing(Option<[f64; 4]>);

impl Easing {
    pub const LINEAR: Self = Self(None);

    pub fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64) -> Option<Self> {
        ((0.0..=1.0).contains(&x1) && (0.0..=1.0).contains(&x2) && y1.is_finite() && y2.is_finite())
            .then_some(Self(Some([x1, y1, x2, y2])))
    }

    fn sample(self, progress: f64) -> f64 {
        let Some([x1, y1, x2, y2]) = self.0 else {
            return progress;
        };
        if progress == 0.0 || progress == 1.0 {
            return progress;
        }
        let coordinate = |p1: f64, p2: f64, t: f64| {
            let inverse = 1.0 - t;
            (3.0 * inverse * inverse * t) * p1 + (3.0 * inverse * t * t) * p2 + t * t * t
        };
        // The x axis is monotonic. Bisection also handles horizontal tangents
        // and curves whose two x control points coincide.
        let (mut low, mut high) = (0.0, 1.0);
        for _ in 0..24 {
            let mid = (low + high) * 0.5;
            if coordinate(x1, x2, mid) < progress {
                low = mid;
            } else {
                high = mid;
            }
        }
        coordinate(y1, y2, (low + high) * 0.5)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Animation {
    duration: Duration,
    delay: Duration,
    easing: Easing,
}

impl Animation {
    pub fn new(duration_ms: u64, delay_ms: u64, easing: Easing) -> Self {
        Self {
            duration: Duration::from_millis(duration_ms),
            delay: Duration::from_millis(delay_ms),
            easing,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StateTiming {
    pub case: i64,
    pub enter: Option<Animation>,
    pub exit: Option<Animation>,
}

struct Track {
    from: Value,
    begin: Duration,
    end: Duration,
    easing: Easing,
}

impl Track {
    fn sample(&self, to: &Value, now: Duration) -> Option<Value> {
        if now >= self.end {
            return None;
        }
        if now <= self.begin {
            return Some(self.from.clone());
        }
        let progress = (now - self.begin).as_secs_f64() / (self.end - self.begin).as_secs_f64();
        interpolate(&self.from, to, self.easing.sample(progress))
    }
}

type Tracks = BTreeMap<Vec<usize>, BTreeMap<&'static str, Track>>;

#[derive(Default)]
pub struct SceneTransition {
    tracks: Tracks,
}

impl SceneTransition {
    pub fn between(previous: &Node, next: &Node) -> Self {
        let mut result = Self::default();
        result.retarget(previous, next, Duration::ZERO);
        result
    }

    /// Replace the targets at this instant on the same scene clock.
    /// Unchanged targets keep their original start and end times.
    pub fn retarget(&mut self, previous: &Node, next: &Node, now: Duration) {
        fn collect(
            previous: &Node,
            next: &Node,
            path: &mut Vec<usize>,
            old: &mut Tracks,
            new: &mut Tracks,
            now: Duration,
        ) {
            if previous.kind != next.kind
                || previous.element_id() != next.element_id()
                || previous.row_key != next.row_key
            {
                return;
            }
            let mut old_at_path = old.remove(path.as_slice()).unwrap_or_default();
            let mut tracks = BTreeMap::new();
            let mut track = |property, timing: Option<Animation>| {
                if timing.is_some_and(|timing| timing.duration.is_zero() && timing.delay.is_zero())
                {
                    return;
                }
                let (Some(previous_target), Some(next_target)) = (
                    previous.properties.get(property),
                    next.properties.get(property),
                ) else {
                    return;
                };
                let old = old_at_path.remove(property).filter(|track| now < track.end);
                if previous_target == next_target {
                    if let Some(old) = old {
                        tracks.insert(property, old);
                    }
                    return;
                }
                let Some(timing) = timing else {
                    return;
                };
                let from = old
                    .and_then(|track| track.sample(previous_target, now))
                    .unwrap_or_else(|| previous_target.clone());
                if from != *next_target && interpolate(&from, next_target, 0.0).is_some() {
                    let begin = now.saturating_add(timing.delay);
                    tracks.insert(
                        property,
                        Track {
                            from,
                            begin,
                            end: begin.saturating_add(timing.duration),
                            easing: timing.easing,
                        },
                    );
                }
            };
            for (&property, &timing) in &next.animations {
                track(property, Some(timing));
            }
            if let Some(timings) = &next.state_timings {
                for (&property, next_state) in timings.iter() {
                    let previous_state = previous
                        .state_timings
                        .as_ref()
                        .and_then(|timings| timings.get(property));
                    let timing = previous_state
                        .filter(|state| state.case != next_state.case)
                        .and_then(|state| next_state.enter.or(state.exit));
                    track(property, timing);
                }
            }
            if !tracks.is_empty() {
                new.insert(path.clone(), tracks);
            }
            for (index, child) in next.materialized_children() {
                let Some(previous) = previous.child_at(index) else {
                    continue;
                };
                path.push(index);
                collect(previous, child, path, old, new, now);
                path.pop();
            }
        }
        let mut old = std::mem::take(&mut self.tracks);
        collect(
            previous,
            next,
            &mut Vec::new(),
            &mut old,
            &mut self.tracks,
            now,
        );
    }

    /// Return the earliest useful frame time. Pending delays can sleep until
    /// their start time. A running interpolation needs another frame now.
    pub fn next_frame(&self, elapsed: Duration) -> Option<Duration> {
        self.tracks
            .values()
            .flat_map(|tracks| tracks.values())
            .filter(|track| elapsed < track.end)
            .map(|track| track.begin.max(elapsed))
            .min()
    }

    pub fn running(&self, elapsed: Duration) -> bool {
        self.next_frame(elapsed).is_some()
    }

    pub fn frame<'a>(&self, target: &'a Node, elapsed: Duration) -> Cow<'a, Node> {
        if !self.running(elapsed) {
            return Cow::Borrowed(target);
        }
        let mut frame = target.clone();
        for (path, tracks) in &self.tracks {
            if tracks.values().all(|track| elapsed >= track.end) {
                continue;
            }
            let mut node = Some(&mut frame);
            for &index in path {
                node = node.and_then(|node| node.child_at_mut(index));
            }
            let Some(node) = node else {
                continue;
            };
            for (&property, track) in tracks {
                if let Some(value) = node.properties.get_mut(property)
                    && let Some(interpolated) = track.sample(value, elapsed)
                {
                    *value = interpolated;
                }
            }
        }
        Cow::Owned(frame)
    }
}

fn mix(a: f64, b: f64, amount: f64) -> f64 {
    a * (1.0 - amount) + b * amount
}

fn mix_color(a: Color, b: Color, amount: f64) -> Color {
    let channels = [24, 16, 8, 0].map(|shift| {
        mix(
            f64::from((a.0 >> shift) & 255),
            f64::from((b.0 >> shift) & 255),
            amount,
        )
        .round()
        .clamp(0.0, 255.0) as u32
    });
    let [r, g, b, a] = channels;
    Color((r << 24) | (g << 16) | (b << 8) | a)
}

fn mix_stops(a: &[GradientStop], b: &[GradientStop], amount: f64) -> Option<Arc<[GradientStop]>> {
    if a.len() != b.len() {
        return None;
    }
    let position_progress = amount.clamp(0.0, 1.0);
    Some(
        a.iter()
            .zip(b)
            .map(|(a, b)| GradientStop {
                offset: mix(a.offset, b.offset, position_progress),
                color: mix_color(a.color, b.color, amount),
            })
            .collect(),
    )
}

fn interpolate(from: &Value, to: &Value, amount: f64) -> Option<Value> {
    if !amount.is_finite() {
        return None;
    }
    let number = |a: f64, b: f64| {
        let result = mix(a, b, amount);
        result.is_finite().then_some(result)
    };
    Some(match (from, to) {
        (Value::Int(a), Value::Int(b)) => {
            Value::Int(mix(*a as f64, *b as f64, amount).round() as i64)
        }
        (Value::Float(a), Value::Float(b)) => Value::Float(number(*a, *b)?),
        (Value::Length(Length(a)), Value::Length(Length(b))) => {
            Value::Length(Length(number(*a, *b)?))
        }
        (Value::Percent(Percent(a)), Value::Percent(Percent(b))) => {
            Value::Percent(Percent(number(*a, *b)?))
        }
        (Value::Color(a) | Value::Brush(Brush::Solid(a)), Value::Color(b)) => {
            Value::Color(mix_color(*a, *b, amount))
        }
        (Value::Color(a) | Value::Brush(Brush::Solid(a)), Value::Brush(Brush::Solid(b))) => {
            Value::Brush(Brush::Solid(mix_color(*a, *b, amount)))
        }
        (
            Value::Brush(Brush::Linear {
                angle_degrees: a,
                stops: from,
            }),
            Value::Brush(Brush::Linear {
                angle_degrees: b,
                stops: to,
            }),
        ) => Value::Brush(Brush::Linear {
            angle_degrees: number(*a, *b)?,
            stops: mix_stops(from, to, amount)?,
        }),
        (
            Value::Brush(Brush::Radial { stops: from }),
            Value::Brush(Brush::Radial { stops: to }),
        ) => Value::Brush(Brush::Radial {
            stops: mix_stops(from, to, amount)?,
        }),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    fn rectangle(width: f64, timing: Animation) -> Node {
        let mut node = Node::new("Rectangle", Some("bar"));
        node.properties.insert("width", Length(width).into());
        node.animations.insert("width", timing);
        node
    }

    #[test]
    fn unrelated_updates_keep_deadlines_and_retargeting_starts_at_the_visible_value() {
        let timing = Animation::new(200, 100, Easing::LINEAR);
        let initial = rectangle(10.0, timing);
        let target = rectangle(110.0, timing);
        let mut transition = SceneTransition::between(&initial, &target);
        assert_eq!(transition.next_frame(time(20)), Some(time(100)));
        assert_eq!(
            transition.frame(&target, time(20)).length("width"),
            Some(10.0)
        );

        let mut updated = target.clone();
        updated.properties.insert("height", Length(40.0).into());
        transition.retarget(&target, &updated, time(50));
        assert_eq!(transition.next_frame(time(50)), Some(time(100)));
        assert_eq!(
            transition.frame(&updated, time(200)).length("width"),
            Some(60.0)
        );

        let reversed = rectangle(0.0, timing);
        transition.retarget(&updated, &reversed, time(200));
        assert_eq!(transition.next_frame(time(200)), Some(time(300)));
        assert_eq!(
            transition.frame(&reversed, time(200)).length("width"),
            Some(60.0)
        );
        assert_eq!(
            transition.frame(&reversed, time(400)).length("width"),
            Some(30.0)
        );
        assert!(matches!(
            transition.frame(&reversed, time(500)),
            Cow::Borrowed(_)
        ));

        let mut immediate = reversed.clone();
        immediate.animations.clear();
        transition.retarget(&reversed, &immediate, time(400));
        assert_eq!(transition.next_frame(time(400)), None);
        assert_eq!(
            transition.frame(&immediate, time(400)).length("width"),
            Some(0.0)
        );
    }

    #[test]
    fn a_delayed_zero_duration_track_jumps_only_at_its_deadline() {
        let timing = Animation::new(0, 90, Easing::LINEAR);
        let initial = rectangle(20.0, timing);
        let target = rectangle(100.0, timing);
        let transition = SceneTransition::between(&initial, &target);
        assert_eq!(transition.next_frame(time(0)), Some(time(90)));
        assert_eq!(
            transition.frame(&target, time(89)).length("width"),
            Some(20.0)
        );
        assert_eq!(
            transition.frame(&target, time(90)).length("width"),
            Some(100.0)
        );
        assert_eq!(transition.next_frame(time(90)), None);
    }

    #[test]
    fn easing_clamps_overshooting_channels_and_interpolates_matching_gradients() {
        let fast = Easing::cubic_bezier(0.0, 0.8, 1.0, 0.8).unwrap();
        assert!((fast.sample(0.5) - 0.725).abs() < 1e-6);
        let overshoot = Easing::cubic_bezier(0.0, 2.0, 1.0, 2.0)
            .unwrap()
            .sample(0.5);
        assert!(overshoot > 1.0);
        assert_eq!(
            mix_color(Color(0x001020ff), Color(0xeeeeeeff), overshoot),
            Color(0xffffffff)
        );

        let brush = |angle_degrees, start, end, color| {
            Value::Brush(Brush::Linear {
                angle_degrees,
                stops: [
                    GradientStop {
                        color,
                        offset: start,
                    },
                    GradientStop { color, offset: end },
                ]
                .into(),
            })
        };
        let from = brush(0.0, 0.0, 0.8, Color(0xff0000ff));
        let to = brush(90.0, 0.2, 1.0, Color(0x0000ffff));
        assert_eq!(
            interpolate(&from, &to, 0.5),
            Some(brush(45.0, 0.1, 0.9, Color(0x800080ff)))
        );
    }
}
