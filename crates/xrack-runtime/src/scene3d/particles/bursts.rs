//! Prepared one-shot emission events on a particle emitter's timeline.
use std::sync::Arc;

use crate::{
    Point3D, Quaternion3D, RecordValue, Value, VecModel,
    scene3d::particles::track::MAX_EMISSION_TIME,
};

const MAX_BURSTS: usize = 128;
const MAX_BURST_PARTICLES: i64 = 4096;

#[doc(hidden)]
pub struct ParticleBurstDefinition {
    pub time: f64,
    pub count: i64,
    pub duration: f64,
    pub count_variation: i64,
    pub position: Option<Point3D>,
    pub orientation: Quaternion3D,
    pub seed: Option<i64>,
    pub enabled: bool,
}

impl Default for ParticleBurstDefinition {
    fn default() -> Self {
        Self {
            time: 0.0,
            count: 0,
            duration: 0.0,
            count_variation: 0,
            position: None,
            orientation: Quaternion3D::IDENTITY,
            seed: None,
            enabled: true,
        }
    }
}

impl ParticleBurstDefinition {
    pub(crate) fn from_record(row: &RecordValue) -> Option<Self> {
        let (Value::Float(time), Value::Int(count)) = (row.field("time")?, row.field("count")?)
        else {
            return None;
        };
        Some(Self {
            time: *time,
            count: *count,
            duration: match row.field("duration") {
                Some(Value::Float(value)) => *value,
                _ => 0.0,
            },
            count_variation: match row.field("count-variation") {
                Some(Value::Int(value)) => *value,
                _ => 0,
            },
            position: match row.field("position") {
                Some(Value::Point3D(point)) => Some(*point),
                _ => None,
            },
            orientation: match row.field("orientation") {
                Some(Value::Quaternion3D(rotation)) => *rotation,
                _ => Quaternion3D::IDENTITY,
            },
            seed: match row.field("seed") {
                Some(Value::Int(seed)) => Some(*seed),
                _ => None,
            },
            enabled: row.field("enabled") != Some(&Value::Bool(false)),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParticleBurst {
    pub(crate) time: f64,
    pub(crate) duration: f64,
    count: i32,
    variation: i32,
    pub(crate) seed_salt: u32,
    pub(crate) frame: Option<[[f32; 4]; 3]>,
}

impl ParticleBurst {
    pub(crate) fn count(&self, seed: u32) -> u32 {
        if self.variation == 0 {
            return self.count as u32;
        }
        let mut bits = seed ^ self.seed_salt;
        bits = (bits ^ (bits >> 16)).wrapping_mul(0x7feb_352d);
        bits = (bits ^ (bits >> 15)).wrapping_mul(0x846c_a68b);
        bits ^= bits >> 16;
        let offset = (bits % (2 * self.variation + 1) as u32) as i32 - self.variation;
        (self.count + offset).clamp(0, MAX_BURST_PARTICLES as i32) as u32
    }

    pub(crate) fn sample(
        &self,
        elapsed: f64,
        seed: u32,
        lifetime: f64,
        budget: u32,
    ) -> Option<BirthSpan> {
        let count = self.count(seed);
        if count == 0 || elapsed < self.time {
            return None;
        }
        let age = elapsed - self.time;
        let end = count - 1;
        let interval = if end == 0 {
            0.0
        } else {
            self.duration / f64::from(end)
        };
        let (latest, youngest_age) = if interval == 0.0 {
            (end, age)
        } else if age >= self.duration {
            (end, age - self.duration)
        } else {
            let tick = (age / interval).floor().min(f64::from(end - 1));
            (tick as u32, (-tick).mul_add(interval, age).max(0.0))
        };
        if !(0.0..lifetime).contains(&youngest_age) {
            return None;
        }
        let alive = if interval == 0.0 {
            count
        } else {
            (((lifetime - youngest_age) / interval).ceil() as u32).min(latest + 1)
        };
        Some(BirthSpan {
            count: alive.min(budget),
            latest: u64::from(latest),
            youngest_age,
            interval,
            origin: Some(self.time),
            seed_salt: self.seed_salt,
            frame: self.frame,
        })
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParticleBurstSchedule(Arc<[ParticleBurst]>);

impl ParticleBurstSchedule {
    pub(crate) fn from_model<T>(
        rows: &VecModel<T>,
        project: impl FnMut(&T) -> Option<ParticleBurstDefinition>,
    ) -> Option<Self> {
        let (count, drafts) = rows.inspect_rows(0, MAX_BURSTS, project);
        if count > MAX_BURSTS {
            return None;
        }
        let mut bursts = Vec::with_capacity(count);
        for (index, row) in drafts.into_iter().enumerate() {
            let row = row?;
            if !row.enabled {
                continue;
            }
            if !(0.0..=MAX_EMISSION_TIME).contains(&row.time)
                || !(0.0..=MAX_EMISSION_TIME).contains(&row.duration)
                || row.count < 0
                || row.count_variation < 0
            {
                return None;
            }
            if row.count != 0 || row.count_variation != 0 {
                let frame = if let Some(position) = row.position {
                    let position = [position.x, position.y, position.z];
                    if !position.iter().all(|axis| axis.abs() <= MAX_EMISSION_TIME) {
                        return None;
                    }
                    let rotation = row.orientation.rotation_f32();
                    Some(std::array::from_fn(|axis| {
                        std::array::from_fn(|column| {
                            if column == 3 {
                                position[axis] as f32
                            } else {
                                rotation[(axis, column)]
                            }
                        })
                    }))
                } else {
                    None
                };
                // Use the model-local row identity so moving or pruning rows
                // leaves the random streams of the retained events unchanged.
                let id = if let Some(seed) = row.seed {
                    seed as u64
                } else {
                    rows.row_key(index)?.row
                };
                let key = (id ^ (id >> 32)) as u32;
                bursts.push(ParticleBurst {
                    time: row.time,
                    duration: row.duration,
                    count: row.count.min(MAX_BURST_PARTICLES) as i32,
                    variation: row.count_variation.min(MAX_BURST_PARTICLES) as i32,
                    seed_salt: key.wrapping_add(1).wrapping_mul(0x9e37_79b9),
                    frame,
                });
            }
        }
        bursts.sort_by(|a, b| a.time.total_cmp(&b.time));
        Some(Self(bursts.into()))
    }

    pub(crate) fn events(&self) -> &[ParticleBurst] {
        &self.0
    }

    pub(crate) fn has_fixed_placement(&self) -> bool {
        self.0.iter().any(|event| event.frame.is_some())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A suffix of live particles from one stream or scheduled event.
pub(crate) struct BirthSpan {
    pub(crate) count: u32,
    pub(crate) latest: u64,
    pub(crate) youngest_age: f64,
    pub(crate) interval: f64,
    pub(crate) origin: Option<f64>,
    pub(crate) seed_salt: u32,
    pub(crate) frame: Option<[[f32; 4]; 3]>,
}

impl BirthSpan {
    pub(crate) fn continuous(elapsed: f64, rate: f64, lifetime: f64, budget: u32) -> Option<Self> {
        let ticks = elapsed * rate;
        // Beyond this range f64 can no longer distinguish adjacent emissions.
        if !(1.0..(1_u64 << 52) as f64).contains(&ticks) {
            return None;
        }
        let latest = ticks.floor();
        let interval = rate.recip();
        let youngest_age = (ticks - latest) * interval;
        if !(0.0..lifetime).contains(&youngest_age) {
            return None;
        }
        let alive = ((lifetime - youngest_age) * rate).ceil();
        let count = (alive.min(latest) as u32).min(budget);
        (count != 0).then_some(Self {
            count,
            latest: latest as u64,
            youngest_age,
            interval,
            origin: None,
            seed_salt: 0,
            frame: None,
        })
    }

    fn count_at_most(&self, age: f64) -> u32 {
        if age < self.youngest_age || self.count == 0 {
            return 0;
        }
        let age_at = |index| f64::from(index).mul_add(self.interval, self.youngest_age);
        if age_at(self.count - 1) <= age {
            return self.count;
        }
        let mut low = 1;
        let mut high = self.count - 1;
        while low < high {
            let middle = low + (high - low) / 2;
            if age_at(middle) <= age {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low
    }
}

pub(crate) fn retain_newest(
    leading: &mut [Option<BirthSpan>],
    bursts: &mut [BirthSpan],
    budget: u32,
) {
    let spans = || leading.iter().flatten().chain(bursts.iter());
    if spans().map(|span| span.count).sum::<u32>() <= budget {
        return;
    }
    // Find the oldest accepted age without producing per-particle CPU data.
    // Nonnegative f64 values preserve their order as unsigned bit patterns.
    let population = |age| spans().map(|span| span.count_at_most(age)).sum::<u32>();
    let mut low = 0;
    let mut high = f64::INFINITY.to_bits();
    while low < high {
        let middle = low + (high - low) / 2;
        if population(f64::from_bits(middle)) < budget {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    let edge = f64::from_bits(low);
    let younger = edge.next_down();
    let mut tied = budget - population(younger);
    for span in leading.iter_mut().flatten().chain(bursts) {
        let count = span.count_at_most(younger);
        let keep = tied.min(span.count_at_most(edge) - count);
        span.count = count + keep;
        tied -= keep;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_burst_uses_relative_birth_times_and_expires_at_the_lifetime() {
        let burst = ParticleBurst {
            time: 2.0,
            duration: 1.0,
            count: 3,
            variation: 0,
            seed_salt: 1,
            frame: None,
        };
        assert!(burst.sample(2.0_f64.next_down(), 0, 0.75, 8).is_none());
        let first = burst.sample(2.0, 0, 0.75, 8).unwrap();
        assert_eq!((first.count, first.latest, first.youngest_age), (1, 0, 0.0));
        let final_birth = burst.sample(3.0, 0, 0.75, 8).unwrap();
        assert_eq!(
            (
                final_birth.count,
                final_birth.latest,
                final_birth.youngest_age
            ),
            (2, 2, 0.0)
        );
        assert!(burst.sample(3.75, 0, 0.75, 8).is_none());
    }

    #[test]
    fn budget_keeps_newest_births_across_stream_and_overlapping_events() {
        let mut leading = [BirthSpan::continuous(1.5, 4.0, 2.0, 5), None];
        let pulse = |time, duration, count, seed_salt| ParticleBurst {
            time,
            duration,
            count,
            seed_salt,
            variation: 0,
            frame: None,
        };
        leading[1] = pulse(0.5, 1.0, 3, 1).sample(1.5, 0, 2.0, 5);
        let mut bursts = [pulse(1.375, 0.0, 4, 2).sample(1.5, 0, 2.0, 5).unwrap()];
        retain_newest(&mut leading, &mut bursts, 5);
        assert_eq!(leading.map(|span| span.unwrap().count), [1, 1]);
        assert_eq!(bursts[0].count, 3);
        assert_eq!(bursts[0].latest, 3);
    }

    #[test]
    fn event_variation_is_seeded_and_zero_duration_keeps_the_same_birth_age() {
        let pulse = ParticleBurst {
            time: 0.25,
            duration: 0.0,
            count: 17,
            variation: 10,
            seed_salt: 42,
            frame: None,
        };
        let low_budget = pulse.sample(0.5, 13, 2.0, 1).unwrap();
        let full = pulse.sample(0.75, 13, 2.0, 32).unwrap();
        assert!((7..=27).contains(&full.count));
        assert_eq!(full.count, pulse.count(13));
        assert_ne!(pulse.count(1), pulse.count(2));
        assert_eq!(low_budget.latest, full.latest);
        assert_eq!((full.youngest_age, full.interval), (0.5, 0.0));
    }
}
