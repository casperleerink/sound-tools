use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// How a piece is exported. The default is what an export was before there was a choice, and
/// what "Render to audio track" keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub format: Format,
    pub rate: SampleRate,
    pub loudness: Option<Loudness>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    Wav(Depth),
    /// 320 kbps.
    Mp3,
}

impl Default for Format {
    fn default() -> Self {
        Self::Wav(Depth::Float)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Depth {
    /// Dithered.
    Sixteen,
    TwentyFour,
    #[default]
    Float,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SampleRate {
    Hz44100,
    #[default]
    Hz48000,
}

/// A loudness to normalize to, under a true peak of [`CEILING_DBTP`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Loudness {
    /// What the streaming services play at.
    Streaming,
    /// EBU R128.
    Broadcast,
}

/// The highest true peak a normalized export reaches: the room EBU R128 and the streaming
/// services leave for an encoder's own overshoot.
pub const CEILING_DBTP: f64 = -1.0;

impl Loudness {
    pub fn lufs(self) -> f64 {
        match self {
            Self::Streaming => -14.0,
            Self::Broadcast => -23.0,
        }
    }

    const ALL: [Self; 2] = [Self::Streaming, Self::Broadcast];

    fn word(self) -> &'static str {
        match self {
            Self::Streaming => "streaming",
            Self::Broadcast => "broadcast",
        }
    }
}

impl SampleRate {
    pub fn hz(self) -> u32 {
        match self {
            Self::Hz44100 => 44_100,
            Self::Hz48000 => 48_000,
        }
    }

    const ALL: [Self; 2] = [Self::Hz44100, Self::Hz48000];
}

impl Depth {
    fn bits(self) -> u16 {
        match self {
            Self::Sixteen => 16,
            Self::TwentyFour => 24,
            Self::Float => 32,
        }
    }

    const ALL: [Self; 3] = [Self::Sixteen, Self::TwentyFour, Self::Float];
}

impl Format {
    /// What the name of a file of this format ends in.
    pub fn ending(self) -> &'static str {
        match self {
            Self::Wav(_) => "wav",
            Self::Mp3 => "mp3",
        }
    }

    /// Whether `file` is named as a file of this format. The one reading of an ending, for
    /// `--render` and the save panel alike.
    pub fn names(self, file: &Path) -> bool {
        ending(file).as_deref() == Some(self.ending())
    }
}

fn ending(file: &Path) -> Option<String> {
    let ending = file.extension()?.to_str()?;
    Some(ending.to_ascii_lowercase())
}

impl Settings {
    /// The settings `--render <file>` takes from the ending of `file` and from `flags`, and the
    /// flags that are not about the file, in order.
    pub fn parse<'a>(file: &Path, flags: &[&'a str]) -> Result<(Self, Vec<&'a str>)> {
        let (mut rate, mut depth, mut loudness) = (None, None, None);
        let mut rest = Vec::new();
        let mut flags = flags.iter();
        while let Some(flag) = flags.next() {
            let mut value = |name| match flags.next() {
                Some(value) => Ok(*value),
                None => bail!("{name} needs a value"),
            };
            match *flag {
                "--rate" => {
                    let value = value("--rate")?;
                    let found = SampleRate::ALL
                        .into_iter()
                        .find(|rate| value == rate.hz().to_string());
                    let Some(found) = found else {
                        bail!("--rate is 44100 or 48000, not {value}");
                    };
                    rate = Some(found);
                }
                "--depth" => {
                    let value = value("--depth")?;
                    let found = Depth::ALL
                        .into_iter()
                        .find(|depth| value == depth.bits().to_string());
                    let Some(found) = found else {
                        bail!("--depth is 16, 24 or 32, not {value}");
                    };
                    depth = Some(found);
                }
                "--loudness" => {
                    let value = value("--loudness")?;
                    let found = Loudness::ALL
                        .into_iter()
                        .find(|loudness| value == loudness.word());
                    let Some(found) = found else {
                        bail!(
                            "--loudness is streaming (-14 LUFS) or broadcast (-23 LUFS), not {value}"
                        );
                    };
                    loudness = Some(found);
                }
                other => rest.push(other),
            }
        }
        let format = match (ending(file).as_deref(), depth) {
            (Some("wav"), depth) => Format::Wav(depth.unwrap_or_default()),
            (Some("mp3"), None) => Format::Mp3,
            (Some("mp3"), Some(_)) => bail!("an MP3 has no bit depth: leave --depth out"),
            _ => bail!("{} is not a .wav or .mp3 file", file.display()),
        };
        let rate = rate.unwrap_or_default();
        Ok((
            Self {
                format,
                rate,
                loudness,
            },
            rest,
        ))
    }

    /// The flags [`Self::parse`] reads back as these settings, from a file named for its format.
    pub fn arguments(&self) -> Vec<String> {
        let mut arguments = vec!["--rate".to_string(), self.rate.hz().to_string()];
        if let Format::Wav(depth) = self.format {
            arguments.extend(["--depth".to_string(), depth.bits().to_string()]);
        }
        if let Some(loudness) = self.loudness {
            arguments.extend(["--loudness".to_string(), loudness.word().to_string()]);
        }
        arguments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window hands its settings to `--render` as flags: every one of them comes back.
    #[test]
    fn every_setting_reads_back_from_its_flags() {
        let formats = Depth::ALL.map(Format::Wav).into_iter().chain([Format::Mp3]);
        for format in formats {
            for rate in SampleRate::ALL {
                for loudness in [None, Some(Loudness::Streaming), Some(Loudness::Broadcast)] {
                    let settings = Settings {
                        format,
                        rate,
                        loudness,
                    };
                    let file = format!("piece.{}", format.ending());
                    let arguments = settings.arguments();
                    let mut flags: Vec<&str> = arguments.iter().map(String::as_str).collect();
                    flags.extend(["--from", "0", "--no-tail"]);
                    let (read, rest) = Settings::parse(Path::new(&file), &flags).unwrap();
                    assert_eq!(read, settings);
                    assert_eq!(rest, ["--from", "0", "--no-tail"]);
                }
            }
        }
    }

    #[test]
    fn a_render_with_no_flags_is_what_it_always_was() {
        let (settings, _) = Settings::parse(Path::new("piece.WAV"), &[]).unwrap();
        assert_eq!(settings, Settings::default());
        assert!(Settings::parse(Path::new("piece.mp3"), &["--depth", "16"]).is_err());
        assert!(Settings::parse(Path::new("piece.flac"), &[]).is_err());
        assert!(Settings::parse(Path::new("piece"), &[]).is_err());
    }
}
