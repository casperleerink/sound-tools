use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct TempoPoint {
    pub beat: f64,
    pub bpm: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Clock {
    pub tempo: Vec<TempoPoint>,
    pub numerator: u8,
    pub denominator: u8,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            tempo: vec![TempoPoint {
                beat: 0.0,
                bpm: 120.0,
            }],
            numerator: 4,
            denominator: 4,
        }
    }
}

impl Clock {
    pub fn validate(&self) -> Result<()> {
        if self.tempo.is_empty()
            || self.tempo[0].beat != 0.0
            || self.numerator == 0
            || !self.denominator.is_power_of_two()
        {
            return Err(Error(
                "Clock needs a tempo at zero and a valid time signature".into(),
            ));
        }
        let mut previous = -1.0;
        for point in &self.tempo {
            if !point.beat.is_finite()
                || point.beat <= previous
                || !point.bpm.is_finite()
                || !(1.0..=1000.0).contains(&point.bpm)
            {
                return Err(Error(
                    "Tempo points must be ordered with finite positive tempos".into(),
                ));
            }
            previous = point.beat;
        }
        Ok(())
    }
    pub fn seconds_at_beat(&self, beat: f64) -> f64 {
        let mut seconds = 0.0;
        for (index, point) in self.tempo.iter().enumerate() {
            let end = self
                .tempo
                .get(index + 1)
                .map_or(beat, |next| next.beat.min(beat));
            if end > point.beat {
                seconds += (end - point.beat) * 60.0 / point.bpm;
            }
            if end >= beat {
                break;
            }
        }
        seconds
    }
    pub fn beat_at_seconds(&self, seconds: f64) -> f64 {
        let mut remaining = seconds.max(0.0);
        for (index, point) in self.tempo.iter().enumerate() {
            if let Some(next) = self.tempo.get(index + 1) {
                let duration = (next.beat - point.beat) * 60.0 / point.bpm;
                if remaining >= duration {
                    remaining -= duration;
                    continue;
                }
            }
            return point.beat + remaining * point.bpm / 60.0;
        }
        0.0
    }
    pub fn frame_at_beat(&self, beat: f64, sample_rate: f64) -> u64 {
        (self.seconds_at_beat(beat) * sample_rate).round() as u64
    }
    pub fn beat_at_frame(&self, frame: u64, sample_rate: f64) -> f64 {
        self.beat_at_seconds(frame as f64 / sample_rate)
    }
    pub fn beats_per_bar(&self) -> f64 {
        f64::from(self.numerator) * 4.0 / f64::from(self.denominator)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Transport {
    pub engine_frame: u64,
    pub project_frame: u64,
    pub playing: bool,
    pub revision: u64,
}

impl Transport {
    pub fn play(&mut self) {
        self.playing = true;
    }
    pub fn pause(&mut self) {
        self.playing = false;
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn stop(&mut self) {
        self.pause();
        self.project_frame = 0;
    }
    pub fn seek(&mut self, frame: u64) {
        self.project_frame = frame;
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn advance(&mut self, frames: usize) {
        self.engine_frame = self.engine_frame.saturating_add(frames as u64);
        if self.playing {
            self.project_frame = self.project_frame.saturating_add(frames as u64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tempo_changes_round_trip() {
        let clock = Clock {
            tempo: vec![
                TempoPoint {
                    beat: 0.0,
                    bpm: 120.0,
                },
                TempoPoint {
                    beat: 4.0,
                    bpm: 60.0,
                },
            ],
            ..Clock::default()
        };
        clock.validate().unwrap();
        assert_eq!(clock.frame_at_beat(8.0, 48000.0), 288000);
        for beat in [0.0, 1.25, 4.0, 6.75, 100.0] {
            assert!((clock.beat_at_seconds(clock.seconds_at_beat(beat)) - beat).abs() < 1e-9);
        }
    }
    #[test]
    fn stopped_audio_keeps_advancing_engine_time() {
        let mut transport = Transport::default();
        transport.advance(128);
        assert_eq!(transport.project_frame, 0);
        transport.play();
        transport.advance(128);
        transport.pause();
        transport.advance(128);
        assert_eq!(transport.engine_frame, 384);
        assert_eq!(transport.project_frame, 128);
        transport.seek(500);
        assert!(!transport.playing);
        transport.play();
        transport.seek(250);
        assert!(transport.playing);
        transport.stop();
        assert_eq!(transport.project_frame, 0);
    }
}
