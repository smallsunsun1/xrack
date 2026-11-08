use crate::{Color, Length, Node, Percent, Value};
use std::{borrow::Cow, time::Duration};

struct Track {
    path: Vec<usize>,
    property: &'static str,
    from: Value,
    duration: Duration,
}

#[derive(Default)]
pub struct SceneTransition {
    tracks: Vec<Track>,
}

impl SceneTransition {
    pub fn between(previous: &Node, next: &Node) -> Self {
        fn collect(previous: &Node, next: &Node, path: &mut Vec<usize>, tracks: &mut Vec<Track>) {
            if previous.kind != next.kind || previous.id != next.id {
                return;
            }
            for (&property, &milliseconds) in &next.animations {
                if milliseconds > 0
                    && let (Some(from), Some(to)) = (
                        previous.properties.get(property),
                        next.properties.get(property),
                    )
                    && from != to
                    && interpolate(from, to, 0.0).is_some()
                {
                    tracks.push(Track {
                        path: path.clone(),
                        property,
                        from: from.clone(),
                        duration: Duration::from_millis(milliseconds),
                    });
                }
            }
            for (index, (previous, next)) in
                previous.children.iter().zip(&next.children).enumerate()
            {
                path.push(index);
                collect(previous, next, path, tracks);
                path.pop();
            }
        }
        let mut tracks = Vec::new();
        collect(previous, next, &mut Vec::new(), &mut tracks);
        Self { tracks }
    }

    pub fn running(&self, elapsed: Duration) -> bool {
        self.tracks.iter().any(|track| track.duration > elapsed)
    }

    pub fn frame<'a>(&self, target: &'a Node, elapsed: Duration) -> Cow<'a, Node> {
        if !self.running(elapsed) {
            return Cow::Borrowed(target);
        }
        let mut frame = target.clone();
        for track in &self.tracks {
            if elapsed >= track.duration {
                continue;
            }
            let mut node = Some(&mut frame);
            for index in &track.path {
                node = node.and_then(|node| node.children.get_mut(*index));
            }
            if let Some(value) = node.and_then(|node| node.properties.get_mut(track.property))
                && let Some(interpolated) = interpolate(
                    &track.from,
                    value,
                    elapsed.as_secs_f64() / track.duration.as_secs_f64(),
                )
            {
                *value = interpolated;
            }
        }
        Cow::Owned(frame)
    }
}

fn interpolate(from: &Value, to: &Value, amount: f64) -> Option<Value> {
    let lerp = |a: f64, b: f64| a * (1.0 - amount) + b * amount;
    Some(match (from, to) {
        (Value::Int(a), Value::Int(b)) => Value::Int(lerp(*a as f64, *b as f64).round() as i64),
        (Value::Float(a), Value::Float(b)) if a.is_finite() && b.is_finite() => {
            Value::Float(lerp(*a, *b))
        }
        (Value::Length(Length(a)), Value::Length(Length(b))) if a.is_finite() && b.is_finite() => {
            Value::Length(Length(lerp(*a, *b)))
        }
        (Value::Percent(Percent(a)), Value::Percent(Percent(b)))
            if a.is_finite() && b.is_finite() =>
        {
            Value::Percent(Percent(lerp(*a, *b)))
        }
        (Value::Color(Color(a)), Value::Color(Color(b))) => {
            let channels = [24, 16, 8, 0].map(|shift| {
                lerp(f64::from((a >> shift) & 255), f64::from((b >> shift) & 255)).round() as u32
            });
            let [r, g, b, a] = channels;
            Value::Color(Color((r << 24) | (g << 16) | (b << 8) | a))
        }
        _ => return None,
    })
}
