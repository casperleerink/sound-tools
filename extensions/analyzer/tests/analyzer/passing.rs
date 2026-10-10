//! The analyzer leaves the sound as it is, to the bit, and shows the card what passed.

use sound_core::Scope;

use crate::support::{SAMPLE_RATE, noise, render, rig};

/// Noise, two seconds of silence, in which the analyzer comes to rest, and noise again: every
/// sample comes out as it went in, also the smallest, and the scope has the last frames.
#[test]
fn the_sound_passes_bit_for_bit_and_into_the_scope() {
    let second = SAMPLE_RATE as usize;
    let mut sound = noise(0.9, 1, second);
    sound.extend(vec![[0.0; 2]; 2 * second]);
    sound.push([f32::MIN_POSITIVE, -f32::MIN_POSITIVE]);
    sound.extend(noise(0.5, 2, second));
    let (mut engine, scopes) = rig(1, |_| sound.clone());
    let output = render(&mut engine, sound.len());
    let bits = |frames: &[[f32; 2]]| -> Vec<[u32; 2]> {
        frames.iter().map(|frame| frame.map(f32::to_bits)).collect()
    };
    assert_eq!(bits(&output), bits(&sound));
    let mut seen = Vec::new();
    scopes[0].read(0, &mut seen);
    assert_eq!(bits(&seen), bits(&sound[sound.len() - Scope::FRAMES..]));
}
