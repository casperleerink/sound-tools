//! The shared voice engine: which voice a new note takes, the pedal, `AllOff`, mono, legato and
//! glide. Played with a voice that only records what it was told.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

use proptest::prelude::*;
use sound_notes::{NoteEvent, Pedal, Pitch, Velocity, Voice, Voices};

#[derive(Clone, Debug, Default, PartialEq)]
struct Recorded {
    /// Started and not ended by the test.
    sounding: bool,
    /// Started and not released or cut.
    held: bool,
    pitch: f32,
    velocity: u8,
    /// How often a note started on this voice.
    starts: u32,
}

impl Voice for Recorded {
    type Context = ();

    fn is_idle(&self) -> bool {
        !self.sounding
    }

    fn loudness(&self) -> f32 {
        f32::from(self.velocity)
    }

    fn start(&mut self, pitch: f32, velocity: Velocity, _: &()) {
        *self = Self {
            sounding: true,
            held: true,
            pitch,
            velocity: velocity.value(),
            starts: self.starts + 1,
        };
    }

    fn set_pitch(&mut self, pitch: f32, _: &()) {
        self.pitch = pitch;
    }

    fn release(&mut self) {
        self.held = false;
    }
}

fn voices<const N: usize>(polyphony: usize) -> Voices<Recorded, N> {
    Voices::new(Recorded::default(), polyphony)
}

fn on<const N: usize>(voices: &mut Voices<Recorded, N>, key: u8, velocity: u8) {
    let event = NoteEvent::On {
        pitch: Pitch::new(key).unwrap(),
        velocity: Velocity::new(velocity).unwrap(),
    };
    voices.handle(event, &());
}

fn off<const N: usize>(voices: &mut Voices<Recorded, N>, key: u8) {
    let pitch = Pitch::new(key).unwrap();
    voices.handle(NoteEvent::Off { pitch }, &());
}

fn pedal<const N: usize>(voices: &mut Voices<Recorded, N>, value: u8) {
    voices.handle(NoteEvent::Pedal(Pedal::new(value).unwrap()), &());
}

/// The pitch of every voice, idle or not, in slot order.
fn pitches<const N: usize>(voices: &Voices<Recorded, N>) -> Vec<f32> {
    voices.iter().map(|voice| voice.pitch).collect()
}

/// The pitches of the voices that are held.
fn held<const N: usize>(voices: &Voices<Recorded, N>) -> Vec<f32> {
    let held = voices.iter().filter(|voice| voice.held);
    held.map(|voice| voice.pitch).collect()
}

#[test]
fn a_new_note_takes_an_idle_voice_then_the_quietest_released_then_the_oldest_held() {
    let mut voices = voices::<4>(4);
    on(&mut voices, 60, 100);
    on(&mut voices, 62, 50);
    on(&mut voices, 64, 80);
    on(&mut voices, 65, 90);
    assert_eq!(pitches(&voices), [60.0, 62.0, 64.0, 65.0]);
    off(&mut voices, 62);
    off(&mut voices, 64);
    on(&mut voices, 67, 100);
    on(&mut voices, 69, 100);
    // The quietest released voice first, then the other released one.
    assert_eq!(pitches(&voices), [60.0, 67.0, 69.0, 65.0]);
    // None is released now: the oldest held one gives way.
    on(&mut voices, 71, 100);
    assert_eq!(pitches(&voices), [71.0, 67.0, 69.0, 65.0]);
    let starts: Vec<u32> = voices.iter().map(|voice| voice.starts).collect();
    assert_eq!(starts, [2, 2, 2, 1]);
}

/// With fewer notes than voices, a voice that is taken over is cut and sounds out in its
/// place, and the new note takes an idle voice, or else the cut one closest to silence.
#[test]
fn a_cut_voice_sounds_out_beside_the_new_note() {
    let mut voices = voices::<3>(2);
    on(&mut voices, 60, 20);
    on(&mut voices, 62, 100);
    on(&mut voices, 64, 100);
    assert_eq!(pitches(&voices), [60.0, 62.0, 64.0]);
    assert_eq!(held(&voices), [62.0, 64.0]);
    assert!(voices.iter().all(|voice| voice.sounding));
    // 62 is cut now too, and the quieter of the two cut voices makes room.
    on(&mut voices, 65, 100);
    assert_eq!(pitches(&voices), [65.0, 62.0, 64.0]);
    assert_eq!(held(&voices), [65.0, 64.0]);
    assert_eq!(voices.newest().map(|voice| voice.pitch), Some(65.0));
}

/// A lower polyphony cuts the oldest notes until the rest fit, and holds from then on.
#[test]
fn a_lower_polyphony_cuts_the_oldest_notes() {
    let mut voices = voices::<4>(4);
    on(&mut voices, 60, 100);
    on(&mut voices, 62, 100);
    on(&mut voices, 64, 100);
    voices.set_polyphony(2);
    assert_eq!(held(&voices), [62.0, 64.0]);
    on(&mut voices, 65, 100);
    assert_eq!(held(&voices), [64.0, 65.0]);
    // A higher one cuts nothing.
    voices.set_polyphony(4);
    assert_eq!(held(&voices), [64.0, 65.0]);
}

#[test]
fn the_pedal_holds_a_released_key_until_it_comes_up() {
    let mut voices = voices::<4>(4);
    pedal(&mut voices, 127);
    on(&mut voices, 60, 100);
    off(&mut voices, 60);
    off(&mut voices, 60);
    assert_eq!(held(&voices), [60.0]);
    // Below the threshold counts as up.
    pedal(&mut voices, 63);
    assert_eq!(held(&voices), [] as [f32; 0]);
    on(&mut voices, 62, 100);
    off(&mut voices, 62);
    assert_eq!(held(&voices), [] as [f32; 0]);
}

#[test]
fn all_off_releases_every_note_and_puts_the_pedal_up() {
    let mut voices = voices::<4>(4);
    voices.set_glide(480.0);
    on(&mut voices, 60, 100);
    pedal(&mut voices, 127);
    on(&mut voices, 62, 100);
    off(&mut voices, 62);
    voices.handle(NoteEvent::AllOff, &());
    assert_eq!(held(&voices), [] as [f32; 0]);
    assert_eq!(voices.pedal(), Pedal::UP);
    // A note after it does not glide from what played before it.
    on(&mut voices, 72, 100);
    assert_eq!(held(&voices), [72.0]);
}

#[test]
fn mono_plays_the_last_key_held() {
    let mut voices = voices::<4>(4);
    voices.set_mono(true);
    on(&mut voices, 60, 100);
    on(&mut voices, 64, 100);
    on(&mut voices, 67, 100);
    assert_eq!(held(&voices), [67.0]);
    off(&mut voices, 67);
    assert_eq!(held(&voices), [64.0]);
    // Not the key that plays: nothing changes.
    off(&mut voices, 60);
    assert_eq!(held(&voices), [64.0]);
    off(&mut voices, 64);
    assert_eq!(held(&voices), [] as [f32; 0]);
    assert_eq!(voices.iter().filter(|voice| voice.sounding).count(), 1);
}

#[test]
fn going_to_mono_cuts_every_note_but_the_newest() {
    let mut voices = voices::<4>(4);
    on(&mut voices, 60, 100);
    on(&mut voices, 64, 100);
    voices.set_mono(true);
    assert_eq!(held(&voices), [64.0]);
    on(&mut voices, 67, 100);
    assert_eq!(held(&voices), [67.0]);
}

#[test]
fn a_legato_moves_the_note_and_does_not_start_it_again() {
    let mut voices = voices::<4>(4);
    voices.set_mono(true);
    on(&mut voices, 60, 100);
    on(&mut voices, 64, 10);
    let sounding: Vec<_> = voices.iter().filter(|voice| voice.sounding).collect();
    assert_eq!(sounding.len(), 1);
    assert_eq!((sounding[0].pitch, sounding[0].velocity), (64.0, 100));
    assert_eq!(sounding[0].starts, 1);
    // With no key held the next note starts again, and the one before sounds out released.
    off(&mut voices, 64);
    off(&mut voices, 60);
    on(&mut voices, 62, 90);
    assert_eq!(held(&voices), [62.0]);
    assert_eq!(voices.iter().map(|voice| voice.starts).sum::<u32>(), 2);
}

/// Renders `frames` in stretches of 64, as an instrument does between events.
fn glide<const N: usize>(voices: &mut Voices<Recorded, N>, frames: usize) {
    for stretch in (0..frames).step_by(64) {
        voices.glide((frames - stretch).min(64), &());
    }
}

#[test]
fn a_glide_reaches_the_key_in_the_glide_time() {
    let mut voices = voices::<4>(4);
    voices.set_glide(4_800.0);
    on(&mut voices, 60, 100);
    off(&mut voices, 60);
    on(&mut voices, 72, 100);
    assert_eq!(held(&voices), [60.0]);
    glide(&mut voices, 2_400);
    let halfway = held(&voices)[0];
    assert!((halfway - 66.0).abs() < 1e-3, "{halfway}");
    glide(&mut voices, 2_400);
    let there = held(&voices)[0];
    assert!((there - 72.0).abs() < 1e-3, "{there}");
    glide(&mut voices, 64);
    assert_eq!(held(&voices), [72.0]);

    // In mono a legato glides too, and does not start again.
    voices.set_mono(true);
    on(&mut voices, 60, 100);
    glide(&mut voices, 4_864);
    assert_eq!(held(&voices), [60.0]);
    assert_eq!(voices.iter().map(|voice| voice.starts).sum::<u32>(), 2);
}

#[derive(Clone, Debug)]
enum Step {
    Event(NoteEvent),
    Render(usize),
}

fn step() -> impl Strategy<Value = Step> {
    let key = (60_u8..68).prop_map(|key| Pitch::new(key).unwrap());
    let velocity = (1_u8..=127).prop_map(|value| Velocity::new(value).unwrap());
    prop_oneof![
        4 => (key.clone(), velocity).prop_map(|(pitch, velocity)| Step::Event(NoteEvent::On { pitch, velocity })),
        4 => key.prop_map(|pitch| Step::Event(NoteEvent::Off { pitch })),
        1 => (0_u8..=127).prop_map(|value| Step::Event(NoteEvent::Pedal(Pedal::new(value).unwrap()))),
        1 => Just(Step::Event(NoteEvent::AllOff)),
        1 => (1_usize..=64).prop_map(Step::Render),
    ]
}

proptest! {
    /// Whatever is played, no more notes are held than may play, and once every key and the
    /// pedal are up nothing is held.
    #[test]
    fn no_voice_is_left_stuck(
        steps in prop::collection::vec(step(), 0..200),
        mono in any::<bool>(),
        polyphony in 1_usize..=4,
        glide_frames in prop_oneof![Just(0.0_f32), 1.0_f32..500.0],
    ) {
        let mut voices = voices::<4>(polyphony);
        voices.set_mono(mono);
        voices.set_glide(glide_frames);
        let most = if mono { 1 } else { polyphony };
        for step in steps {
            match step {
                Step::Event(event) => voices.handle(event, &()),
                Step::Render(frames) => voices.glide(frames, &()),
            }
            prop_assert!(held(&voices).len() <= most);
        }
        for key in 60..68 {
            off(&mut voices, key);
        }
        pedal(&mut voices, 0);
        prop_assert_eq!(held(&voices), [] as [f32; 0]);
    }
}
