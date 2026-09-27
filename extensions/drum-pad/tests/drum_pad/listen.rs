//! Renders every sound of the default kit and a short beat to WAV files, for a person to listen
//! to before trying the Drum pad in the window. Run by hand:
//!
//! ```sh
//! cargo nextest run -p drum-pad --run-ignored only listen --no-capture
//! ```

use std::path::Path;

use drum_pad::{DrumPadState, KIT, PADS};

use crate::support::{Harness, QUARTER, hit, note, one_hit, peak};

const FOLDER: &str = "/private/tmp/m4-step-4-sounds";

#[test]
#[ignore = "writes WAV files to listen to; run by hand"]
fn listen_to_every_sound_and_a_beat() {
    let folder = Path::new(FOLDER);
    std::fs::create_dir_all(folder).unwrap();
    for pad in 0..PADS {
        let kit = &KIT[pad];
        let seconds = f64::from(kit.decay_ms) / 1000.0 + 0.25;
        let sound = one_hit(pad, 127, DrumPadState::default(), seconds);
        let name = kit.name.to_lowercase().replace(' ', "-");
        let path = folder.join(format!("{}-{name}.wav", note(pad)));
        sound.write_wav(&path);
        println!(
            "{} peak {:.3} {}",
            path.display(),
            peak(&sound.left).max(peak(&sound.right)),
            kit.name
        );
    }
    let mut harness = Harness::with_track(beat(), DrumPadState::default());
    let beat = harness.play(48_000 * 11);
    let path = folder.join("beat.wav");
    beat.write_wav(&path);
    println!(
        "{} peak {:.3}",
        path.display(),
        peak(&beat.left).max(peak(&beat.right))
    );
}

/// Four bars at 120 bpm and the crash that follows them: a groove on the hats, the same with a
/// ghost note and a clap, a bar on the ride with the pedal hat and the rim, and a fill down the
/// toms.
pub fn beat() -> Vec<sound_notes::Note> {
    const KICK: u8 = 36;
    const RIM: u8 = 37;
    const SNARE: u8 = 38;
    const CLAP: u8 = 39;
    const HAT: u8 = 42;
    const PEDAL: u8 = 44;
    const OPEN: u8 = 46;
    const CRASH: u8 = 49;
    const RIDE: u8 = 51;
    const TOMS: [u8; 6] = [41, 43, 45, 47, 48, 50];
    let sixteenth = QUARTER / 4;
    let mut notes = Vec::new();
    let mut at = |bar: u64, step: u64, note: u8, velocity: u8| {
        notes.push(hit((bar * 16 + step) * sixteenth, note, velocity));
    };
    for bar in 0..4 {
        match bar {
            0 | 1 => {
                for step in (0..16).step_by(2) {
                    if !(bar == 0 && step == 14) {
                        at(bar, step, HAT, if step % 4 == 0 { 105 } else { 78 });
                    }
                }
                if bar == 0 {
                    at(bar, 14, OPEN, 96);
                }
                at(bar, 0, KICK, 120);
                at(bar, 4, SNARE, 112);
                at(bar, 12, SNARE, 112);
                match bar {
                    0 => {
                        at(bar, 8, KICK, 112);
                        at(bar, 10, KICK, 96);
                    }
                    _ => {
                        at(bar, 6, KICK, 100);
                        at(bar, 8, KICK, 112);
                        at(bar, 11, KICK, 92);
                        at(bar, 12, CLAP, 100);
                        at(bar, 15, SNARE, 44);
                    }
                }
            }
            2 => {
                for step in (0..16).step_by(4) {
                    at(bar, step, RIDE, 104);
                    at(bar, step + 2, RIDE, 72);
                    at(bar, step + 2, PEDAL, 90);
                }
                at(bar, 0, KICK, 120);
                at(bar, 7, KICK, 96);
                at(bar, 8, KICK, 110);
                at(bar, 4, RIM, 110);
                at(bar, 12, RIM, 110);
                at(bar, 14, RIM, 70);
            }
            _ => {
                at(bar, 0, KICK, 120);
                at(bar, 0, HAT, 100);
                at(bar, 2, HAT, 76);
                at(bar, 4, SNARE, 112);
                at(bar, 4, HAT, 100);
                at(bar, 6, HAT, 76);
                at(bar, 6, KICK, 100);
                let fill = [5, 5, 4, 3, 2, 2, 1, 0];
                for (index, tom) in fill.into_iter().enumerate() {
                    let velocity = if index % 2 == 0 { 118 } else { 96 };
                    at(bar, 8 + index as u64, TOMS[tom], velocity);
                }
                at(bar, 15, KICK, 100);
            }
        }
    }
    at(4, 0, CRASH, 120);
    at(4, 0, KICK, 124);
    notes
}
