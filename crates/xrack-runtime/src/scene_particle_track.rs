//! Immutable world-space poses sampled at a particle's birth.
use std::sync::Arc;

use crate::{Point3D, Quaternion3D, RecordValue, Value, VecModel, Vector3D};

pub(crate) const MAX_EMISSION_KEYS: usize = 4096;
pub(crate) const MAX_EMISSION_TIME: f64 = 1.0e8;

/// A fixed world placement of a glTF node animation on an emitter's timeline.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleEmissionAnimation {
    pub node: String,
    pub clip: String,
    pub sample_rate: f64,
    pub position: Point3D,
    pub orientation: Quaternion3D,
    pub scale: Vector3D,
}

impl Default for ParticleEmissionAnimation {
    fn default() -> Self {
        Self {
            node: String::new(),
            clip: String::new(),
            sample_rate: 60.0,
            position: Point3D::default(),
            orientation: Quaternion3D::IDENTITY,
            scale: Vector3D::new(1.0, 1.0, 1.0),
        }
    }
}

impl ParticleEmissionAnimation {
    pub(crate) fn from_record(row: &RecordValue) -> Option<Self> {
        let (Value::String(node), Value::String(clip)) = (row.field("node")?, row.field("clip")?)
        else {
            return None;
        };
        let defaults = Self::default();
        Some(Self {
            node: node.clone(),
            clip: clip.clone(),
            sample_rate: match row.field("sample-rate") {
                Some(Value::Float(rate)) => *rate,
                _ => defaults.sample_rate,
            },
            position: match row.field("position") {
                Some(Value::Point3D(position)) => *position,
                _ => defaults.position,
            },
            orientation: match row.field("orientation") {
                Some(Value::Quaternion3D(rotation)) => *rotation,
                _ => defaults.orientation,
            },
            scale: match row.field("scale") {
                Some(Value::Vector3D(scale)) => *scale,
                _ => defaults.scale,
            },
        })
    }
}

/// A fixed-point cycle clock anchored at each wrap of the particle ID.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct ParticleLoopClock {
    step: u64,
    epoch: u64,
    previous: u64,
    pub(crate) duration: f32,
}

impl ParticleLoopClock {
    pub(crate) fn new(rate: f64, latest: u64, duration: f32) -> Self {
        // Both inputs are binary rationals. Divide their exact product into
        // the birth tick so repeated ticks do not accumulate rounded reciprocals.
        let binary = |value: f64| {
            let bits = value.to_bits();
            let exponent = ((bits >> 52) & 2047) as i32;
            let mut mantissa = (bits & ((1_u64 << 52) - 1)) | (u64::from(exponent != 0) << 52);
            let trailing = mantissa.trailing_zeros();
            mantissa >>= trailing;
            (
                u128::from(mantissa),
                exponent.max(1) - 1075 + trailing as i32,
            )
        };
        let (rate, rate_exponent) = binary(rate);
        let (period, period_exponent) = binary(f64::from(duration));
        let exponent = rate_exponent + period_exponent;
        // The mantissas need at most 77 bits. A positive exponent still
        // fits because the rate and the duration are each bounded by 1e8.
        let divisor = (rate * period) << exponent.max(0) as u32;

        let phase = |tick: u64| {
            let mut remainder = u128::from(tick) % divisor;
            let mut shift = (-exponent).max(0) as u32;
            while shift > 0 {
                let bits = shift.min(32);
                remainder = (remainder << bits) % divisor;
                shift -= bits;
            }
            let high = (remainder << 32) / divisor;
            remainder = (remainder << 32) % divisor;
            let low = ((remainder << 32) + divisor / 2) / divisor;
            ((high as u64) << 32).wrapping_add(low as u64)
        };
        let wrap = latest & !u64::from(u32::MAX);
        Self {
            step: phase(1),
            epoch: phase(wrap),
            previous: phase(wrap.saturating_sub(1_u64 << 32)),
            duration,
        }
    }

    pub(crate) fn for_burst(time: f64, interval: f64, duration: f32) -> Self {
        // A burst has at most 4096 births and its tick never wraps.
        let period = f64::from(duration);
        let phase = |seconds: f64| {
            (seconds.rem_euclid(period) / period * 18_446_744_073_709_551_616.0).round() as u64
        };
        let origin = phase(time);
        Self {
            step: phase(interval),
            epoch: origin,
            previous: origin,
            duration,
        }
    }

    /// Encode an unsigned 64-bit phase as two words for the particle shader.
    pub fn words(self) -> [[u32; 4]; 3] {
        let pack = |phase: u64, duration| [phase as u32, (phase >> 32) as u32, 0, duration];
        [
            pack(self.step, self.duration.to_bits()),
            pack(self.epoch, 0),
            pack(self.previous, 0),
        ]
    }

    pub(crate) fn time(self, id: u32, latest: u32) -> f32 {
        let origin = if id > latest {
            self.previous
        } else {
            self.epoch
        };
        let phase = origin.wrapping_add(self.step.wrapping_mul(u64::from(id)));
        let upper = (phase >> 48) as f32 * (1.0 / 65_536.0);
        let lower = ((phase >> 32) as u32 & 65_535) as f32 * (1.0 / 4_294_967_296.0)
            + phase as u32 as f32 * (1.0 / 18_446_744_073_709_551_616.0);
        let main = upper * self.duration;
        let residual = upper.mul_add(self.duration, -main);
        let time = main + lower.mul_add(self.duration, residual);
        if time < self.duration { time } else { 0.0 }
    }
}

/// GPU-ready keyframes with positions, relative times, and unit quaternions.
#[doc(hidden)]
#[derive(Clone, Debug, Default)]
pub struct ParticleEmissionTrack(Arc<[[f32; 8]]>);

impl PartialEq for ParticleEmissionTrack {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

impl ParticleEmissionTrack {
    pub(crate) fn from_model<T>(
        rows: &VecModel<T>,
        project: impl FnMut(&T) -> Option<(f64, Point3D, Quaternion3D)>,
    ) -> Option<Self> {
        let (count, keys) = rows.inspect_rows(0, MAX_EMISSION_KEYS, project);
        if count > MAX_EMISSION_KEYS {
            return None;
        }
        Self::from_frames(keys.into_iter())
    }

    pub(crate) fn from_frames(
        keys: impl ExactSizeIterator<Item = Option<(f64, Point3D, Quaternion3D)>>,
    ) -> Option<Self> {
        let mut previous = -1.0_f32;
        let mut prepared = Vec::with_capacity(keys.len());
        for key in keys {
            let (time, point, orientation) = key?;
            let point = [point.x, point.y, point.z];
            if !(0.0..=MAX_EMISSION_TIME).contains(&time)
                || !point.iter().all(|axis| axis.abs() <= 1.0e8)
                || time as f32 <= previous
            {
                return None;
            }
            previous = time as f32;
            let [x, y, z] = point.map(|value| value as f32);
            let [qx, qy, qz, qw] = orientation.components().map(|value| value as f32);
            prepared.push([x, y, z, previous, qx, qy, qz, qw]);
        }
        Some(Self(prepared.into()))
    }

    pub fn keys(&self) -> &[[f32; 8]] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn duration(&self) -> f32 {
        self.0.last().map_or(0.0, |key| key[3])
    }

    /// Bound the part of the path used by the currently alive particles.
    pub(crate) fn position_bounds(&self, [earliest, latest]: [f32; 2]) -> Option<[[f64; 3]; 2]> {
        let positions = |key: &[f32; 8]| [key[0], key[1], key[2]].map(f64::from);
        let points = |time| self.0.partition_point(|key| key[3] <= time);
        let interpolate = |time: f32, index: usize| {
            let before = self.0.get(index.saturating_sub(1))?;
            let a = positions(before);
            let Some(after) = self.0.get(index).filter(|_| index > 0) else {
                return Some(a);
            };
            let b = positions(after);
            let factor = f64::from((time - before[3]) / (after[3] - before[3]));
            Some(std::array::from_fn(|axis| {
                a[axis] + (b[axis] - a[axis]) * factor
            }))
        };
        let first = points(earliest);
        let last = points(latest);
        let start = interpolate(earliest, first)?;
        let end = interpolate(latest, last)?;
        let mut result = [
            std::array::from_fn(|axis| start[axis].min(end[axis])),
            std::array::from_fn(|axis| start[axis].max(end[axis])),
        ];
        for key in &self.0[first..last] {
            for (axis, point) in positions(key).into_iter().enumerate() {
                result[0][axis] = result[0][axis].min(point);
                result[1][axis] = result[1][axis].max(point);
            }
        }
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::ParticleLoopClock;

    #[test]
    fn birth_phase_survives_particle_id_wrap_and_later_samples() {
        let before = ParticleLoopClock::new(8.0, u64::from(u32::MAX), 0.75);
        let after = ParticleLoopClock::new(8.0, (1_u64 << 32) + 3, 0.75);
        for id in (u32::MAX - 3)..=u32::MAX {
            let expected = ((f64::from(id) * 0.125) % 0.75) as f32;
            assert!((before.time(id, u32::MAX) - expected).abs() < 1e-6);
            assert_eq!(before.time(id, u32::MAX), after.time(id, 3));
        }
        assert_eq!(after.time(0, 3), 0.5);
        assert_eq!(after.time(3, 3), 0.125);
    }

    #[test]
    fn an_emission_at_the_cycle_boundary_returns_to_zero() {
        let clock = ParticleLoopClock::new(3.0, 3, 1.0);
        assert_eq!(clock.time(3, 3), 0.0);
        assert!((clock.time(1, 3) - (1.0 / 3.0)).abs() < f32::EPSILON);
        let wrapped = ParticleLoopClock::new(3.0, 1_u64 << 32, 1.0);
        assert_eq!(wrapped.time(u32::MAX, 0), 0.0);
        assert_eq!(wrapped.time(0, 0), 1.0 / 3.0);
    }
}
