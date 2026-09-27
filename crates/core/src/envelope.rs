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
    sustain: f64,
    release_coefficient: f64,
    release_base: f64,
}

impl Envelope {
    /// Times in seconds, `sustain` as a part of full level. A time under one frame takes one.
    pub fn new(
        attack_seconds: f32,
        decay_seconds: f32,
        sustain: f32,
        release_seconds: f32,
        sample_rate: f32,
    ) -> Self {
        // The factor that covers a distance of 1 in `seconds`, when the curve aims `overshoot`
        // past the end of that distance.
        let coefficient = |seconds: f32, overshoot: f64| {
            let frames = (f64::from(seconds) * f64::from(sample_rate)).max(1.0);
            (-((1.0 + overshoot) / overshoot).ln() / frames).exp()
        };
        let attack_coefficient = coefficient(attack_seconds, ATTACK_OVERSHOOT);
        let release_coefficient = coefficient(release_seconds, ENVELOPE_FLOOR);
        Self {
            attack_coefficient,
            attack_base: (1.0 + ATTACK_OVERSHOOT) * (1.0 - attack_coefficient),
            decay_coefficient: coefficient(decay_seconds, ENVELOPE_FLOOR),
            sustain: f64::from(sustain),
            release_coefficient,
            release_base: -ENVELOPE_FLOOR * (1.0 - release_coefficient),
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
                self.level = envelope.sustain + above * envelope.decay_coefficient;
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
            assert!((settled as f64 - expected).abs() <= 1.0, "decay {decay}: {settled}");
            // From full level, the release reaches silence in its time.
            state.level = 1.0;
            state.release();
            let silent = frames_until(&mut state, &envelope, EnvelopeState::is_idle);
            let expected = (release * RATE).round() as usize;
            assert!(silent.abs_diff(expected) <= 1, "release {release}: {silent}");
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
