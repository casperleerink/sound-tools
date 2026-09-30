//! An envelope of four stages, attack, decay to a sustain level and release, for an instrument
//! that shapes each voice. Next to [`Smoothed`](crate::Smoothed), the other helper a processor
//! uses per frame. The core itself shapes nothing and knows no notes: an instrument starts and
//! releases a voice when it decides to.

/// How far past full level the attack aims. It shapes the attack curve, and lets it reach full
/// level in exactly the attack time.
const ATTACK_OVERSHOOT: f64 = 0.3;

/// -60 dB. The release aims this far below silence, and so reaches silence in exactly the
/// release time from full level. The decay aims this far past the sustain level, so it is
/// within 0.1 % of the way there after `ln 1000 / ln 1001` of the decay time. A held voice with
/// no sustain ends when it falls below this.
pub const ENVELOPE_FLOOR: f64 = 0.001;

/// How each stage of a [`curved`](Envelope::curved) envelope bends, from 0, straight to the
/// eye, to 1, a strong exponential curve: fast at first and slow near its end, like the release
/// of an analog envelope. A value between gives a curve in between.
///
/// A curve is a stage that aims past its end, reached in exactly the stage time. At 1 it aims
/// 0.1 % of its distance past, as the release of [`Envelope::new`] does, at 0.5 as far again
/// as its distance, and at 0 a thousand times its distance, which is straight to within
/// 0.02 % of full level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvelopeCurves {
    pub attack: f32,
    pub decay: f32,
    pub release: f32,
}

impl EnvelopeCurves {
    /// How far past its end a stage with this curve aims, as a part of its distance.
    fn overshoot(curve: f32) -> f64 {
        ENVELOPE_FLOOR.powf(2.0 * f64::from(curve.clamp(0.0, 1.0)) - 1.0)
    }

    /// How far a stage with this curve is on its way after `part` of its time, both from 0 to
    /// 1: the shape of the stage, for a view to draw.
    pub fn progress(curve: f32, part: f32) -> f32 {
        let overshoot = Self::overshoot(curve);
        let left = (overshoot / (1.0 + overshoot)).powf(f64::from(part.clamp(0.0, 1.0)));
        ((1.0 + overshoot) * (1.0 - left)) as f32
    }
}

/// The envelope at one sample rate, as per-frame factors. Each stage is `level * coefficient +
/// base`: a curve toward a point a little past its target. In `f64`: in `f32` a decay of 0.4 s
/// came 9 frames late in 19200, a slow pole losing its last digits.
///
/// Work it out when the times change, on the control side or in `update`; it costs three `exp`
/// and three `ln`. A change applies at once to every voice on it, held ones too.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Envelope {
    attack_coefficient: f64,
    attack_base: f64,
    decay_coefficient: f64,
    /// What the decay moves toward the sustain each frame on top of its curve, so it aims past
    /// the sustain and stops on it. 0 for the analog shape of [`new`](Self::new), which glides
    /// toward the sustain and never quite reaches it.
    decay_drop: f64,
    sustain: f64,
    release_coefficient: f64,
    release_base: f64,
}

impl Envelope {
    /// The analog shape the synth and the Sampler play. Times in seconds, `sustain` as a part
    /// of full level. A time under one frame takes one.
    ///
    /// The attack aims 30 % past full level and reaches it in the attack time. The release aims
    /// 0.1 % of full level below silence and reaches silence in the release time from full
    /// level. The decay glides toward the sustain and is within 0.1 % of the way there after
    /// `ln 1000 / ln 1001` of the decay time.
    pub fn new(
        attack_seconds: f32,
        decay_seconds: f32,
        sustain: f32,
        release_seconds: f32,
        sample_rate: f32,
    ) -> Self {
        let overshoots = [ATTACK_OVERSHOOT, ENVELOPE_FLOOR, ENVELOPE_FLOOR];
        let times = [attack_seconds, decay_seconds, release_seconds];
        Self {
            decay_drop: 0.0,
            ..Self::aiming_past(times, sustain, overshoots, sample_rate)
        }
    }

    /// An envelope whose stages bend as `curves` says. Each stage reaches its end in exactly
    /// its time: the attack full level, the decay the sustain, and the release silence from
    /// full level.
    pub fn curved(
        attack_seconds: f32,
        decay_seconds: f32,
        sustain: f32,
        release_seconds: f32,
        curves: EnvelopeCurves,
        sample_rate: f32,
    ) -> Self {
        let overshoots =
            [curves.attack, curves.decay, curves.release].map(EnvelopeCurves::overshoot);
        let times = [attack_seconds, decay_seconds, release_seconds];
        Self::aiming_past(times, sustain, overshoots, sample_rate)
    }

    /// Each stage of `times`, attack, decay and release, aims the matching part of
    /// `overshoots` of its distance past its end, and gets there in its time.
    fn aiming_past(
        [attack_seconds, decay_seconds, release_seconds]: [f32; 3],
        sustain: f32,
        [attack_overshoot, decay_overshoot, release_overshoot]: [f64; 3],
        sample_rate: f32,
    ) -> Self {
        // The factor that covers a distance of 1 in `seconds`, when the curve aims `overshoot`
        // past the end of that distance.
        let coefficient = |seconds: f32, overshoot: f64| {
            let frames = (f64::from(seconds) * f64::from(sample_rate)).max(1.0);
            (-((1.0 + overshoot) / overshoot).ln() / frames).exp()
        };
        let attack_coefficient = coefficient(attack_seconds, attack_overshoot);
        let decay_coefficient = coefficient(decay_seconds, decay_overshoot);
        let release_coefficient = coefficient(release_seconds, release_overshoot);
        let sustain = f64::from(sustain);
        Self {
            attack_coefficient,
            attack_base: (1.0 + attack_overshoot) * (1.0 - attack_coefficient),
            decay_coefficient,
            decay_drop: decay_overshoot * (1.0 - sustain) * (1.0 - decay_coefficient),
            sustain,
            release_coefficient,
            release_base: -release_overshoot * (1.0 - release_coefficient),
        }
    }
}

/// Where a voice is on its envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvelopeStage {
    Idle,
    Attack,
    /// Decay, and the sustain it ends in. One stage, so a sustain edit on a held voice glides
    /// there at the speed of the decay.
    Decay,
    Release,
}

/// One voice on an envelope: its stage and its level, from 0 to 1. `Copy`, so a voice keeps one
/// by value. An instrument may set the level itself, such as a voice that is taken over and
/// keeps its loudness, and then [`start`](Self::start) it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvelopeState {
    pub stage: EnvelopeStage,
    pub level: f64,
}

impl EnvelopeState {
    pub const IDLE: Self = Self {
        stage: EnvelopeStage::Idle,
        level: 0.0,
    };

    pub fn is_idle(&self) -> bool {
        self.stage == EnvelopeStage::Idle
    }

    /// In its attack or its decay and sustain: the key is down.
    pub fn is_held(&self) -> bool {
        matches!(self.stage, EnvelopeStage::Attack | EnvelopeStage::Decay)
    }

    /// Starts the attack from the level the voice has, or the decay when that is full level or
    /// more, such as a loud voice taken over by a quiet note.
    pub fn start(&mut self) {
        self.stage = if self.level < 1.0 {
            EnvelopeStage::Attack
        } else {
            EnvelopeStage::Decay
        };
    }

    /// Goes to the release, when held.
    pub fn release(&mut self) {
        if self.is_held() {
            self.stage = EnvelopeStage::Release;
        }
    }

    /// One frame on: the level of this frame. It is idle, at level 0, once its release reaches
    /// silence, or when a held voice with no sustain has decayed.
    #[inline]
    pub fn next(&mut self, envelope: &Envelope) -> f64 {
        match self.stage {
            EnvelopeStage::Attack => {
                self.level = self.level * envelope.attack_coefficient + envelope.attack_base;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvelopeStage::Decay;
                }
            }
            EnvelopeStage::Decay => {
                let above = self.level - envelope.sustain;
                // The drop points toward the sustain, also from below it after a sustain edit.
                let next = above * envelope.decay_coefficient - envelope.decay_drop.copysign(above);
                // A curve that aims past the sustain stops on it.
                let next = if (next < 0.0) == (above < 0.0) {
                    next
                } else {
                    0.0
                };
                self.level = envelope.sustain + next;
                // A pluck: with no sustain a held voice ends here, and not at its release.
                if self.level < ENVELOPE_FLOOR && envelope.sustain < ENVELOPE_FLOOR {
                    *self = Self::IDLE;
                }
            }
            EnvelopeStage::Release => {
                self.level = self.level * envelope.release_coefficient + envelope.release_base;
                if self.level <= 0.0 {
                    *self = Self::IDLE;
                }
            }
            EnvelopeStage::Idle => {}
        }
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.;

    /// Frames from the start until `done` holds, the first frame counted as 1.
    fn frames_until(
        state: &mut EnvelopeState,
        envelope: &Envelope,
        done: impl Fn(&EnvelopeState) -> bool,
    ) -> usize {
        (1..10_000_000)
            .find(|_| {
                state.next(envelope);
                done(state)
            })
            .unwrap_or(0)
    }

    /// The shape a view draws is the attack the envelope plays.
    #[test]
    fn the_progress_of_a_stage_is_where_the_envelope_is() {
        for curve in [0.0, 0.5, 1.0] {
            let curves = EnvelopeCurves {
                attack: curve,
                decay: curve,
                release: curve,
            };
            let envelope = Envelope::curved(0.1, 0.1, 0.5, 0.1, curves, RATE);
            let mut state = EnvelopeState::IDLE;
            state.start();
            for frame in 1..=4_800 {
                let level = state.next(&envelope) as f32;
                if frame % 480 == 0 {
                    let drawn = EnvelopeCurves::progress(curve, frame as f32 / 4_800.);
                    assert!(
                        (level - drawn).abs() < 1e-3,
                        "{curve} {frame}: {level} {drawn}"
                    );
                }
            }
        }
        assert!((EnvelopeCurves::progress(0.0, 0.5) - 0.5).abs() < 1e-3);
        assert_eq!(EnvelopeCurves::progress(1.0, 1.0), 1.0);
    }

    #[test]
    fn each_stage_takes_its_time() {
        for (attack, decay, sustain, release) in [
            (0.001, 0.05, 0.25, 0.001),
            (0.05, 0.4, 0.55, 0.3),
            (0.4, 2.0, 0.8, 1.5),
        ] {
            let envelope = Envelope::new(attack, decay, sustain, release, RATE);
            let mut state = EnvelopeState::IDLE;
            state.start();
            let full = frames_until(&mut state, &envelope, |state| state.level >= 1.0);
            // Exactly on the last frame of the attack the level is 1 less a rounding error.
            let expected = (attack * RATE).round() as usize;
            assert!(full.abs_diff(expected) <= 1, "attack {attack}: {full}");
            let near = 0.001 * (1.0 - f64::from(sustain));
            let settled = frames_until(&mut state, &envelope, |state| {
                state.level - f64::from(sustain) <= near
            });
            let expected = f64::from(decay * RATE) * 1000_f64.ln() / 1001_f64.ln();
            assert!(
                (settled as f64 - expected).abs() <= 1.0,
                "decay {decay}: {settled}"
            );
            // From full level, the release reaches silence in its time.
            state.level = 1.0;
            state.release();
            let silent = frames_until(&mut state, &envelope, EnvelopeState::is_idle);
            let expected = (release * RATE).round() as usize;
            assert!(
                silent.abs_diff(expected) <= 1,
                "release {release}: {silent}"
            );
            assert_eq!(state, EnvelopeState::IDLE);
        }
    }

    #[test]
    fn a_held_voice_with_no_sustain_ends_after_its_decay_and_a_loud_one_starts_in_it() {
        let envelope = Envelope::new(0.001, 0.1, 0.0, 0.3, RATE);
        let mut state = EnvelopeState::IDLE;
        state.start();
        let ended = frames_until(&mut state, &envelope, EnvelopeState::is_idle);
        assert!((4_800..4_900).contains(&ended), "{ended}");
        let mut loud = EnvelopeState {
            stage: EnvelopeStage::Release,
            level: 1.5,
        };
        loud.start();
        assert_eq!(loud.stage, EnvelopeStage::Decay);
        assert!(loud.next(&envelope) < 1.5);
    }
}
