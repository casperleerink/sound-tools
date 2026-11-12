//! A value typed on a control: how the number a composer types is the value of the control, in
//! the unit the control shows. The field opens on the value without its unit, and a unit that
//! is typed counts: `2k` on a frequency, `1.5 s` on a time in milliseconds.

/// The names of the notes of an octave, from C.
const NOTES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
/// The semitones of each note letter above C, from `a`.
const SEMITONES: [i32; 7] = [9, 11, 0, 2, 4, 5, 7];

/// How a typed number is a value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Typed {
    /// As it stands, whatever follows it: `-6 dB`, `4:1`, `+3 st`.
    Number,
    /// This many are one of the value: 100 for a part shown in percent, so `50%` is 0.5.
    Per(f32),
    /// A frequency in hertz: `1.2 kHz` and `1.2k` are 1200.
    Hertz,
    /// A time in milliseconds: `1.5 s` is 1500.
    Milliseconds,
    /// A time in seconds: `120 ms` is 0.12.
    Seconds,
    /// From -1, left, to 1, right: `C`, `25L`, `100R`.
    Pan,
    /// The number of a MIDI note: `C2` is 36, as is `36`.
    Note,
}

impl Typed {
    /// What the field opens on: the value as [`Self::value`] reads it back, without its unit.
    pub fn text(self, value: f32) -> String {
        match self {
            Self::Number | Self::Hertz | Self::Milliseconds | Self::Seconds => plain(value),
            Self::Per(one) => plain(value * one),
            Self::Pan if value == 0. => "C".into(),
            Self::Pan => {
                let side = if value < 0. { 'L' } else { 'R' };
                format!("{}{side}", plain(value.abs() * 100.))
            }
            Self::Note => {
                let note = value.round() as i32;
                let name = NOTES.get(note.rem_euclid(12) as usize).copied();
                format!("{}{}", name.unwrap_or_default(), note.div_euclid(12) - 1)
            }
        }
    }

    /// The value `text` says, or `None` when it says no number.
    pub fn value(self, text: &str) -> Option<f32> {
        let text: String = (text.chars())
            .filter(|letter| !letter.is_whitespace())
            .map(|letter| match letter {
                '\u{2212}' => '-',
                letter => letter.to_ascii_lowercase(),
            })
            .collect();
        if self == Self::Note
            && let Some(note) = note(&text)
        {
            return Some(note);
        }
        if self == Self::Pan && text == "c" {
            return Some(0.);
        }
        let (number, unit) = number(&text)?;
        let value = match self {
            Self::Number | Self::Note => number,
            Self::Per(one) => number / f64::from(one),
            Self::Hertz if unit.starts_with('k') => number * 1_000.,
            Self::Milliseconds if unit == "s" => number * 1_000.,
            Self::Seconds if unit == "ms" => number / 1_000.,
            Self::Hertz | Self::Milliseconds | Self::Seconds => number,
            Self::Pan if unit == "l" => -number.abs() / 100.,
            Self::Pan => number / 100.,
        };
        Some(value as f32)
    }
}

/// A number with three decimals at most and no zeros at its end: `1200`, `0.25`, `-4.5`.
fn plain(value: f32) -> String {
    let text = format!("{value:.3}");
    let text = match text.contains('.') {
        true => text.trim_end_matches('0').trim_end_matches('.'),
        false => &text,
    };
    match text {
        "-0" => "0".into(),
        text => text.into(),
    }
}

/// The number `text` starts with, and what follows it.
fn number(text: &str) -> Option<(f64, &str)> {
    let digits = |text: &str| text.bytes().take_while(u8::is_ascii_digit).count();
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let whole = digits(unsigned);
    let part = (unsigned.get(whole..)?.strip_prefix('.')).map_or(0, digits);
    if whole + part == 0 {
        return None;
    }
    // A point with nothing after it is not read: `2.` is 2.
    let length = text.len() - unsigned.len() + whole + part + usize::from(part > 0);
    let (number, rest) = text.split_at_checked(length)?;
    Some((number.parse().ok()?, rest.trim_start_matches('.')))
}

/// The number of the note `text` names, such as `f#3` or `bb-1`.
fn note(text: &str) -> Option<f32> {
    let mut letters = text.chars();
    let letter = letters
        .next()
        .filter(|letter| ('a'..='g').contains(letter))?;
    let rest = letters.as_str();
    let (accidental, octave) = match rest.strip_prefix('#') {
        Some(octave) => (1, octave),
        None => match rest.strip_prefix('b') {
            Some(octave) => (-1, octave),
            None => (0, rest),
        },
    };
    let octave: i32 = octave.parse().ok()?;
    let semitones = SEMITONES.get(letter as usize - 'a' as usize)?;
    Some((semitones + accidental + (octave + 1) * 12) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_read_in_the_unit_it_shows() {
        assert_eq!(Typed::Hertz.value("1.2 kHz"), Some(1_200.));
        assert_eq!(Typed::Hertz.value("2k"), Some(2_000.));
        assert_eq!(Typed::Hertz.value("440"), Some(440.));
        assert_eq!(Typed::Hertz.value("632 Hz"), Some(632.));
        assert_eq!(Typed::Number.value("-6 dB"), Some(-6.));
        assert_eq!(Typed::Number.value("\u{2212}4.5 dB"), Some(-4.5));
        assert_eq!(Typed::Number.value("+3 st"), Some(3.));
        assert_eq!(Typed::Number.value("4:1"), Some(4.));
        assert_eq!(Typed::Number.value(".5"), Some(0.5));
        assert_eq!(Typed::Number.value("2."), Some(2.));
        assert_eq!(Typed::Per(100.).value("50%"), Some(0.5));
        assert_eq!(Typed::Per(100.).value("33.3"), Some(0.333));
        assert_eq!(Typed::Per(180.).value("90°"), Some(0.5));
        assert_eq!(Typed::Milliseconds.value("120 ms"), Some(120.));
        assert_eq!(Typed::Milliseconds.value("1.5 s"), Some(1_500.));
        assert_eq!(Typed::Milliseconds.value("250"), Some(250.));
        assert_eq!(Typed::Seconds.value("120 ms"), Some(0.12));
        assert_eq!(Typed::Seconds.value("1.5 s"), Some(1.5));
        assert_eq!(Typed::Seconds.value("2"), Some(2.));
        assert_eq!(Typed::Pan.value("C"), Some(0.));
        assert_eq!(Typed::Pan.value("25L"), Some(-0.25));
        assert_eq!(Typed::Pan.value("100R"), Some(1.));
        assert_eq!(Typed::Pan.value("-30"), Some(-0.3));
        assert_eq!(Typed::Note.value("C2"), Some(36.));
        assert_eq!(Typed::Note.value("f#3"), Some(54.));
        assert_eq!(Typed::Note.value("Bb-1"), Some(10.));
        assert_eq!(Typed::Note.value("60"), Some(60.));
    }

    #[test]
    fn words_that_say_no_number_are_no_value() {
        for unit in [Typed::Number, Typed::Hertz, Typed::Pan, Typed::Note] {
            assert_eq!(unit.value(""), None);
            assert_eq!(unit.value("loud"), None);
            assert_eq!(unit.value("-"), None);
            assert_eq!(unit.value("."), None);
            // Rust reads these as numbers, a knob does not.
            assert_eq!(unit.value("inf"), None);
            assert_eq!(unit.value("nan"), None);
        }
        assert_eq!(Typed::Number.value("C2"), None);
        assert_eq!(Typed::Note.value("h2"), None);
    }

    #[test]
    fn the_field_opens_on_what_it_reads_back() {
        let opens = [
            (Typed::Hertz, 1_234.5, "1234.5"),
            (Typed::Number, -4.5, "-4.5"),
            (Typed::Number, 0., "0"),
            (Typed::Number, -0.0001, "0"),
            (Typed::Per(100.), 0.3, "30"),
            (Typed::Per(100.), 0.125, "12.5"),
            (Typed::Milliseconds, 1_500., "1500"),
            (Typed::Seconds, 0.015, "0.015"),
            (Typed::Pan, 0., "C"),
            (Typed::Pan, -0.25, "25L"),
            (Typed::Pan, 1., "100R"),
            (Typed::Note, 36., "C2"),
            (Typed::Note, 61., "C#4"),
            (Typed::Note, 0., "C-1"),
        ];
        for (unit, value, text) in opens {
            assert_eq!(unit.text(value), text);
            let back = unit.value(text);
            assert!(
                back.is_some_and(|back| (back - value).abs() < 0.001),
                "{text}"
            );
        }
    }
}
