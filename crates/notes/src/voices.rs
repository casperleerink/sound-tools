//! The note handling every bundled instrument shares: which voice plays which key, which voice
//! a new note takes, the sustain pedal, `AllOff`, poly and mono, glide and legato.
//!
//! The instrument owns the sound of a voice: its oscillators, envelope and filter. [`Voices`]
//! owns when a voice starts, moves and ends, and tells the voice through [`Voice`].
//!
//! A pitch here is in semitones on the MIDI scale, 60 is middle C and 69 is A4. It is a whole
//! number except while it glides.

use sound_core::Smoothed;

use crate::{NoteEvent, Pedal, Pitch, Velocity};

/// The frequency of a pitch in semitones, with A4 at 440 Hz.
pub fn frequency_hz(pitch: f32) -> f32 {
    440.0 * ((pitch - 69.0) / 12.0).exp2()
}

/// One voice of an instrument: the sound of one note. [`Voices`] decides when it starts, moves
/// and ends.
pub trait Voice {
    /// What the voice reads from its instrument to start or move, such as the sample rate.
    type Context;

    /// It makes no sound, so a new note may take it without cutting anything.
    fn is_idle(&self) -> bool;

    /// How loud it is now. Of the released voices, a new note takes the quietest.
    fn loudness(&self) -> f32;

    /// Starts a note. On a voice that still sounds this is a takeover, and how that sounds is
    /// the voice's choice: the synth keeps its phase and loudness, so it does not click.
    fn start(&mut self, pitch: f32, velocity: Velocity, context: &Self::Context);

    /// Moves the pitch of the note it plays without starting it again: a glide or a legato.
    fn set_pitch(&mut self, pitch: f32, context: &Self::Context);

    /// The key is up and no pedal holds it.
    fn release(&mut self);

    /// Another note takes the place of this one. The voice should end soon; until it is idle it
    /// sounds on beside the new note and does not count against the polyphony. A voice that
    /// takes over in place only needs the release, the default. One that cannot, such as a
    /// sample that jumps to its start, fades out quickly instead.
    fn cut(&mut self) {
        self.release();
    }
}

/// Where the key of a voice is.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Key {
    Down,
    /// Up, and the sustain pedal holds the note until it comes up.
    Pedal,
    Up,
}

struct Slot<V> {
    voice: V,
    /// None until the first note.
    key: Option<Pitch>,
    held: Key,
    /// Taken over by another note: it sounds out and does not count against the polyphony.
    cut: bool,
    /// The count of the note on that started it. The lowest is the oldest.
    started: u64,
    /// In semitones, on its way to the key while it glides.
    pitch: Smoothed,
}

impl<V: Voice> Slot<V> {
    fn is_idle(&self) -> bool {
        self.voice.is_idle()
    }

    /// It sounds and counts against the polyphony.
    fn is_playing(&self) -> bool {
        !self.cut && !self.is_idle()
    }

    fn release(&mut self) {
        self.held = Key::Up;
        self.voice.release();
    }

    fn cut(&mut self) {
        self.cut = true;
        self.voice.cut();
    }

    /// The key came up. With the pedal down the note sounds on until the pedal comes up.
    fn key_up(&mut self, pedal_is_down: bool) {
        if pedal_is_down && self.held != Key::Up {
            self.held = Key::Pedal;
        } else {
            self.release();
        }
    }
}

/// The quietest of `slots`, the first of them when several are as quiet.
fn quietest<'a, V: Voice + 'a>(
    slots: impl Iterator<Item = (usize, &'a Slot<V>)>,
) -> Option<(usize, &'a Slot<V>)> {
    slots.min_by(|(_, a), (_, b)| a.voice.loudness().total_cmp(&b.voice.loudness()))
}

/// `N` voices and the notes they play. Everything is allocated here, so nothing on the audio
/// thread allocates.
///
/// - Poly: up to `polyphony` notes play at once. One more takes over a voice: the quietest
///   released one, or the oldest held one when none is released. That voice is cut
///   ([`Voice::cut`]). The new note gets an idle voice, or else the cut voice closest to
///   silence, which with `polyphony` equal to `N` is the one just cut.
/// - Mono: one note plays. A key while another is held moves the note there without starting
///   it again (legato), and letting go of it goes back to the last key still held.
/// - Glide: a new note, or a legato, slides from where the last note is to its key in the
///   glide time. None by default.
/// - Pedal: a key that comes up while the pedal is down sounds on until the pedal comes up.
/// - `AllOff` releases everything and puts the pedal up. Nothing is ever stuck.
pub struct Voices<V, const N: usize> {
    slots: [Slot<V>; N],
    polyphony: usize,
    mono: bool,
    /// Zero for no glide.
    glide_frames: f32,
    /// Up after every `AllOff`, so a stop, a seek or an edit can leave no note hanging under a
    /// pedal nobody will lift.
    pedal: Pedal,
    /// For each key while it is down, the count of the note on that pressed it; 0 while it is
    /// up. Mono goes back to the highest.
    pressed: [u64; 128],
    notes_started: u64,
    /// The slot of the note started or moved last: the voice mono plays, and where a glide
    /// starts. None after `AllOff`, so what plays after a stop or a seek never glides from
    /// what played before it.
    newest: Option<usize>,
}

impl<V: Voice + Clone, const N: usize> Voices<V, N> {
    /// `N` copies of `idle`, of which `polyphony`, from 1 to `N`, play at once.
    pub fn new(idle: V, polyphony: usize) -> Self {
        const { assert!(N > 0, "an instrument needs at least one voice") };
        Self {
            slots: std::array::from_fn(|_| Slot {
                voice: idle.clone(),
                key: None,
                held: Key::Up,
                cut: false,
                started: 0,
                pitch: Smoothed::new(0.0),
            }),
            polyphony: polyphony.clamp(1, N),
            mono: false,
            glide_frames: 0.0,
            pedal: Pedal::UP,
            pressed: [0; 128],
            notes_started: 0,
            newest: None,
        }
    }
}

impl<V: Voice, const N: usize> Voices<V, N> {
    /// To mono, every note but the newest is cut, so only one plays on.
    pub fn set_mono(&mut self, mono: bool) {
        if mono && !self.mono {
            let newest = self.newest;
            let slots = self.slots.iter_mut().enumerate();
            let others = slots.filter(|(index, slot)| Some(*index) != newest && slot.is_playing());
            others.for_each(|(_, slot)| slot.cut());
        }
        self.mono = mono;
    }

    /// How many notes may play at once in poly, from 1 to `N`. When fewer may play than do,
    /// the oldest are cut until they fit, as going to mono cuts all but the newest.
    pub fn set_polyphony(&mut self, polyphony: usize) {
        self.polyphony = polyphony.clamp(1, N);
        while self.slots.iter().filter(|slot| slot.is_playing()).count() > self.polyphony {
            let playing = self.slots.iter_mut().filter(|slot| slot.is_playing());
            if let Some(oldest) = playing.min_by_key(|slot| slot.started) {
                oldest.cut();
            }
        }
    }

    /// How long a glide takes, in frames. Zero for none.
    pub fn set_glide(&mut self, frames: f32) {
        self.glide_frames = frames.max(0.0);
    }

    /// Where the sustain pedal stands.
    pub fn pedal(&self) -> Pedal {
        self.pedal
    }

    /// No voice sounds.
    pub fn is_idle(&self) -> bool {
        self.slots.iter().all(Slot::is_idle)
    }

    /// Every voice, idle or not, always in the same order.
    pub fn iter(&self) -> impl Iterator<Item = &V> {
        self.slots.iter().map(|slot| &slot.voice)
    }

    /// Every voice, idle or not, always in the same order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.slots.iter_mut().map(|slot| &mut slot.voice)
    }

    /// The voice of the newest note that plays and was not cut.
    pub fn newest(&self) -> Option<&V> {
        let playing = self.slots.iter().filter(|slot| slot.is_playing());
        playing
            .max_by_key(|slot| slot.started)
            .map(|slot| &slot.voice)
    }

    /// Cuts every voice that sounds, such as when the sound they play is replaced.
    pub fn cut_all(&mut self) {
        let sounding = self.slots.iter_mut().filter(|slot| !slot.is_idle());
        sounding.for_each(Slot::cut);
    }

    /// Follows one event. The wheels and the pressure are not notes and change nothing here.
    pub fn handle(&mut self, event: NoteEvent, context: &V::Context) {
        match event {
            NoteEvent::On { pitch, velocity } => self.key_down(pitch, velocity, context),
            NoteEvent::Off { pitch } => self.key_up(pitch, context),
            NoteEvent::Pedal(value) => {
                // Half pedal is kept as it was played but not acted on: one damper.
                if self.pedal.is_down() && !value.is_down() {
                    let held = self.slots.iter_mut().filter(|slot| slot.held == Key::Pedal);
                    held.for_each(Slot::release);
                }
                self.pedal = value;
            }
            NoteEvent::AllOff => {
                self.pedal = Pedal::UP;
                self.pressed = [0; 128];
                self.newest = None;
                self.slots.iter_mut().for_each(Slot::release);
            }
            NoteEvent::Bend(_) | NoteEvent::ModWheel(_) | NoteEvent::Pressure(_) => {}
        }
    }

    /// Moves every glide `frames` on. Call it after rendering those frames, so each stretch
    /// plays at the pitch where it starts, like the wheels.
    pub fn glide(&mut self, frames: usize, context: &V::Context) {
        for slot in self.slots.iter_mut().filter(|slot| slot.pitch.is_moving()) {
            let pitch = slot.pitch.advance(frames);
            slot.voice.set_pitch(pitch, context);
        }
    }

    fn key_down(&mut self, key: Pitch, velocity: Velocity, context: &V::Context) {
        let legato = self.mono && self.pressed.iter().any(|&order| order > 0);
        self.notes_started += 1;
        self.pressed[usize::from(key.number())] = self.notes_started;
        if legato
            && let Some(index) = self.newest
            && self.slots[index].is_playing()
        {
            self.move_to(index, key, context);
            return;
        }
        let from = self.newest.map(|index| self.slots[index].pitch.current());
        let index = self.free_slot();
        let glide_frames = self.glide_frames;
        let slot = &mut self.slots[index];
        slot.key = Some(key);
        slot.held = Key::Down;
        slot.cut = false;
        slot.started = self.notes_started;
        let target = f32::from(key.number());
        slot.pitch = Smoothed::new(target);
        if glide_frames > 0.0
            && let Some(from) = from
        {
            slot.pitch = Smoothed::new(from);
            slot.pitch.set_target(target, glide_frames);
        }
        slot.voice.start(slot.pitch.current(), velocity, context);
        self.newest = Some(index);
    }

    fn key_up(&mut self, key: Pitch, context: &V::Context) {
        self.pressed[usize::from(key.number())] = 0;
        if self.mono
            && let Some(index) = self.newest
            && self.slots[index].key == Some(key)
            && self.slots[index].is_playing()
        {
            // Last note priority: back to the key pressed last of those still down.
            let pressed = self.pressed.iter().enumerate();
            let last = pressed.max_by_key(|(_, order)| **order);
            if let Some((number, &order)) = last
                && order > 0
            {
                self.move_to(index, Pitch::nearest(number as i64), context);
            }
        }
        let pedal_is_down = self.pedal.is_down();
        let of_this_key = self.slots.iter_mut().filter(|slot| slot.key == Some(key));
        of_this_key.for_each(|slot| slot.key_up(pedal_is_down));
    }

    /// Moves the note of a slot to `key` without starting it again: mono's legato.
    fn move_to(&mut self, index: usize, key: Pitch, context: &V::Context) {
        let slot = &mut self.slots[index];
        slot.key = Some(key);
        slot.held = Key::Down;
        let target = f32::from(key.number());
        if self.glide_frames > 0.0 {
            slot.pitch.set_target(target, self.glide_frames);
        } else {
            slot.pitch = Smoothed::new(target);
            slot.voice.set_pitch(target, context);
        }
        self.newest = Some(index);
    }

    /// The slot for a new note. When as many play as may, one is cut first.
    fn free_slot(&mut self) -> usize {
        let polyphony = if self.mono { 1 } else { self.polyphony };
        let playing = || {
            let slots = self.slots.iter().enumerate();
            slots.filter(|(_, slot)| slot.is_playing())
        };
        if playing().count() >= polyphony {
            let released = playing().filter(|(_, slot)| slot.held == Key::Up);
            let oldest = || playing().min_by_key(|(_, slot)| slot.started);
            if let Some((index, _)) = quietest(released).or_else(oldest) {
                self.slots[index].cut();
            }
        }
        let slots = self.slots.iter().enumerate();
        let idle = slots.clone().find(|(_, slot)| slot.is_idle());
        let cut = || quietest(slots.filter(|(_, slot)| slot.cut));
        idle.or_else(cut).map_or(0, |(index, _)| index)
    }
}
