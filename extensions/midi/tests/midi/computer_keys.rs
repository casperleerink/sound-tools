//! The computer keys as a MIDI keyboard: which note a key ends, and how far the octave goes.

use midi::{ComputerKeys, Played};

fn pitch_of(played: Option<Played>) -> Option<u8> {
    match played? {
        Played::On { pitch, .. } | Played::Off { pitch, .. } => Some(pitch.number()),
        _ => None,
    }
}

/// A held key repeats, and the composer may change the octave before letting go. The key
/// still ends the note it started, else that note would sound for ever.
#[test]
fn a_key_ends_the_note_it_started_after_its_repeats_and_an_octave_change() {
    let mut keys = ComputerKeys::default();
    assert_eq!(pitch_of(keys.down("a")), Some(60));
    assert_eq!(keys.down("a"), None, "a repeat started a note again");
    assert_eq!(keys.down("x"), None);
    assert_eq!(pitch_of(keys.down("s")), Some(74));
    assert_eq!(pitch_of(keys.up("a")), Some(60));
    assert_eq!(keys.up("a"), None);
    let released: Vec<_> = keys.release().map(Some).filter_map(pitch_of).collect();
    assert_eq!(released, [74]);
}

/// An octave key that is held repeats. Not every platform marks a repeat, so the keys count
/// one octave per press themselves.
#[test]
fn a_held_octave_key_moves_one_octave_until_it_comes_up() {
    let mut keys = ComputerKeys::default();
    for _ in 0..3 {
        keys.down("x");
    }
    assert_eq!(pitch_of(keys.down("a")), Some(72));
    keys.up("a");
    keys.up("x");
    keys.down("x");
    assert_eq!(pitch_of(keys.down("a")), Some(84));
    // cmd went down, and the key up of `x` will not come.
    keys.release().for_each(drop);
    keys.down("x");
    assert_eq!(pitch_of(keys.down("a")), Some(96));
}

/// macOS names a key held with shift by what it types then, so a key that types something else
/// with shift would come up under another name and its note would never end. Letters keep
/// their name.
#[test]
fn only_letters_play() {
    let others = (' '..='~').filter(|key| !key.is_ascii_lowercase());
    for key in others {
        assert!(!ComputerKeys::plays(&key.to_string()), "{key:?} plays");
    }
}

/// However often an octave key is pressed, each key stays a note of its own.
#[test]
fn at_either_end_of_the_octaves_every_key_is_a_pitch_of_its_own() {
    let rows = "awsedftgyhujkolp";
    for octave_key in ["z", "x"] {
        let mut keys = ComputerKeys::default();
        for _ in 0..20 {
            keys.down(octave_key);
            keys.up(octave_key);
        }
        let pitches: Vec<_> = (rows.chars())
            .map(|key| pitch_of(keys.down(&key.to_string())).unwrap())
            .collect();
        assert!(
            pitches.windows(2).all(|pair| pair[1] == pair[0] + 1),
            "{pitches:?}"
        );
    }
}
