//! How a number reads beside a control: three significant digits at most, in its unit.

/// A pan from -1 (left) to 1 (right): `C`, `25L`, `100R`.
pub(crate) fn pan(pan: f32) -> String {
    let percent = short(pan.abs() * 100.);
    if pan < 0. {
        format!("{percent}L")
    } else if pan > 0. {
        format!("{percent}R")
    } else {
        "C".to_string()
    }
}

/// A number with three significant digits, two decimals at most, and no zeros at its end: `2`,
/// `15.5`, `632`, `0.05`, and a minus sign, not a hyphen, before a negative one: `−4.5`, as wide
/// as the plus. So a value at rest is short, one a hair from 0 reads 0, and a moving one does
/// not jump between widths.
pub(crate) fn short(value: f32) -> String {
    if value == 0. {
        return "0".into();
    }
    let decimals = (2 - value.abs().log10().floor() as i32).clamp(0, 2) as usize;
    let text = format!("{value:.decimals$}");
    let text = match text.contains('.') {
        true => text.trim_end_matches('0').trim_end_matches('.'),
        false => &text,
    };
    match text {
        "-0" => "0".into(),
        text => text.replace('-', "\u{2212}"),
    }
}

/// A frequency: `632 Hz`, `1.2 kHz`.
pub(crate) fn hertz(hz: f32) -> String {
    match hz < 1_000. {
        true => format!("{} Hz", short(hz)),
        false => format!("{} kHz", short(hz / 1_000.)),
    }
}

/// A gain: `-4.5 dB`.
pub(crate) fn decibels(db: f32) -> String {
    format!("{} dB", short(db))
}

/// A part of one as a percentage: `30%`.
pub(crate) fn percent(part: f32) -> String {
    format!("{}%", short(part * 100.))
}

/// A time in milliseconds: `250 ms`, `1.5 s`.
pub(crate) fn milliseconds(ms: f32) -> String {
    match ms < 1_000. {
        true => format!("{} ms", short(ms)),
        false => format!("{} s", short(ms / 1_000.)),
    }
}

/// A pitch in semitones: `-7 st`, `0.25 st`.
pub(crate) fn semitones(semitones: f32) -> String {
    format!("{} st", short(semitones))
}

/// A time in seconds: `5 ms`, `1.5 s`.
pub(crate) fn seconds(seconds: f32) -> String {
    match seconds < 1. {
        true => format!("{} ms", short(seconds * 1_000.)),
        false => format!("{} s", short(seconds)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_readout_has_its_unit_and_three_digits_at_most() {
        assert_eq!(hertz(0.05), "0.05 Hz");
        assert_eq!(hertz(632.), "632 Hz");
        assert_eq!(hertz(999.), "999 Hz");
        assert_eq!(hertz(1_000.), "1 kHz");
        assert_eq!(hertz(2_143.553), "2.14 kHz");
        assert_eq!(decibels(0.), "0 dB");
        assert_eq!(decibels(-4.5), "−4.5 dB");
        assert_eq!(decibels(12.26), "12.3 dB");
        assert_eq!(decibels(3.58e-7), "0 dB");
        assert_eq!(decibels(-3.58e-7), "0 dB");
        assert_eq!(percent(0.), "0%");
        assert_eq!(percent(0.123_456), "12.3%");
        assert_eq!(percent(1.5), "150%");
        assert_eq!(milliseconds(0.1), "0.1 ms");
        assert_eq!(milliseconds(250.), "250 ms");
        assert_eq!(milliseconds(1_500.), "1.5 s");
        assert_eq!(seconds(0.001), "1 ms");
        assert_eq!(seconds(0.0155), "15.5 ms");
        assert_eq!(seconds(1.), "1 s");
        assert_eq!(seconds(60.), "60 s");
        assert_eq!(semitones(-7.), "−7 st");
        assert_eq!(semitones(0.01), "0.01 st");
        assert_eq!(pan(-0.25), "25L");
    }
}
