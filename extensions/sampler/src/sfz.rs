//! SFZ files: the text format most free sampled instruments come in. An SFZ file maps samples
//! to keys and velocities; it holds no audio itself.
//!
//! This reads the part of the format real libraries need to play right: the headers and their
//! inheritance (`<control>`, `<global>`, `<master>`, `<group>`, `<region>`), `#define` and
//! `#include`, key and velocity ranges, round robin (`seq_*`, `lorand`/`hirand`), keyswitches,
//! release samples, loops, level, pitch, the amplitude envelope and choke groups. An opcode it
//! does not know is ignored, as is a value it cannot read: third-party files use many
//! extensions, and a sound that plays a little plainer beats one that is refused.
//!
//! A region that could not play as written is left out rather than played wrong: one that is
//! started by a controller, one that only plays legato, one whose controller conditions do not
//! hold at their defaults, and one that names a generator such as `*sine`.
//!
//! Parsing reads no file itself: [`parse`] gets the text of the main file and a function that
//! gives the text of an included one, so it is tested without a disk.

use std::collections::HashMap;

/// How deep `#include` may nest. A file that includes itself stops here.
const MAX_INCLUDE_DEPTH: usize = 16;

/// One region: a sample and when and how it plays.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    /// The sample, relative to the folder of the SFZ file, with `/` between folders.
    pub sample: String,
    /// The keys and velocities it plays for, both ends included.
    pub keys: (u8, u8),
    pub velocities: (u8, u8),
    /// The key that plays the sample at its own pitch.
    pub keycenter: u8,
    /// Cents per key away from the keycenter: 100 plays in tune, 0 at one pitch, as a drum.
    pub keytrack_cents: f32,
    /// `transpose` and `tune` together.
    pub tune_cents: f32,
    /// The random value of a note must be in `[low, high)` for it to play. `(0, 1)` always.
    pub random: (f32, f32),
    /// Round robin: it plays every `length`-th time its key is played, at `position` (from 1).
    pub sequence: (u32, u32),
    /// The keyswitches it plays under, both ends included. `None` under any.
    pub switch: Option<(u8, u8)>,
    pub trigger: Trigger,
    /// Where it starts and ends in the sample, in frames of the file. The end is the last frame
    /// that plays.
    pub offset: u64,
    pub end: Option<u64>,
    /// `None` takes the loop the file has, if any.
    pub loop_mode: Option<LoopMode>,
    /// The first and the last frame of the loop. `None` takes the file's.
    pub loop_start: Option<u64>,
    pub loop_end: Option<u64>,
    pub volume_db: f32,
    /// A part of full level: `amplitude` in percent over 100.
    pub amplitude: f32,
    /// From -1, left, to 1, right.
    pub pan: f32,
    /// How much the velocity changes the level, from -1 to 1: 1 is the usual, 0 none.
    pub velocity_tracking: f32,
    pub envelope: Adsr,
    /// A note of a region in group `g` silences the sounding regions with `off_by = g`, as a
    /// closed hi-hat cuts an open one. 0 is no group.
    pub group: i64,
    pub off_by: Option<i64>,
    pub off_mode: OffMode,
}

/// The amplitude envelope, in seconds and a part of full level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Adsr {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// When the key goes down.
    Attack,
    /// When the key comes up, at the velocity it went down with: a piano's damper, a string's
    /// release noise.
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    /// Plays once to its end, then stops, held or not.
    NoLoop,
    /// Plays to its end whatever the key does: a drum hit.
    OneShot,
    /// Loops until the envelope ends.
    Continuous,
    /// Loops while the key is down, then plays on to its end.
    Sustain,
}

/// How a region that is choked by another stops.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OffMode {
    /// At once, over a few milliseconds.
    Fast,
    /// Through its own release.
    Normal,
    /// Over this many seconds.
    Time(f32),
}

/// What an SFZ file plays.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sfz {
    pub regions: Vec<Region>,
    /// The keys that switch articulation instead of playing, both ends included.
    pub switch_keys: Option<(u8, u8)>,
    /// The articulation before the first keyswitch is played.
    pub switch_default: Option<u8>,
}

/// Why an SFZ file could not be read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SfzError {
    #[error("it includes {path}, which cannot be read: {reason}")]
    Include { path: String, reason: String },
    #[error(
        "its #include nests deeper than {MAX_INCLUDE_DEPTH}, so a file probably includes itself"
    )]
    TooDeep,
}

/// Reads an SFZ file. `include` gives the text of a file an `#include` names, relative to the
/// folder of the main file, as SFZ players resolve it.
pub fn parse(
    text: &str,
    include: &dyn Fn(&str) -> Result<String, String>,
) -> Result<Sfz, SfzError> {
    let mut reader = Reader {
        include,
        defines: Vec::new(),
        builder: Builder::default(),
    };
    reader.read(text, 0)?;
    Ok(reader.builder.finish())
}

/// Reads the text of the main file and the files it includes, in order.
struct Reader<'a> {
    include: &'a dyn Fn(&str) -> Result<String, String>,
    /// `$NAME` and its value, the longest name first, so `$VEL10` is not read as `$VEL1`
    /// and a `0`.
    defines: Vec<(String, String)>,
    builder: Builder,
}

impl Reader<'_> {
    /// Headers, opcodes, `#define` and `#include` may come anywhere, also several on one line:
    /// `<region> #define $KEY 21 lokey=21 #include "sample.txt"`. A define holds from where it
    /// is on, also in the files included after it, and an included file is read in its place.
    fn read(&mut self, text: &str, depth: usize) -> Result<(), SfzError> {
        if depth > MAX_INCLUDE_DEPTH {
            return Err(SfzError::TooDeep);
        }
        let text = without_comments(text);
        let mut rest = text.trim_start();
        while !rest.is_empty() {
            rest = if let Some(after) = rest.strip_prefix("#define") {
                let (name, after) = word(after);
                let (value, after) = word(after);
                // A define's own name is not put in, so `$EXT_TEST` can follow `$EXT`.
                if name.starts_with('$') && !value.is_empty() {
                    let value = self.substituted(value);
                    self.defines.retain(|(defined, _)| defined != name);
                    self.defines.push((name.to_string(), value));
                    self.defines
                        .sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
                }
                after
            } else if let Some(after) = rest.strip_prefix("#include") {
                let after = after.trim_start();
                let (path, after) = match after.strip_prefix('"') {
                    Some(quoted) => match quoted.split_once('"') {
                        Some((path, after)) => (path, after),
                        None => (quoted.lines().next().unwrap_or_default(), ""),
                    },
                    None => word(after),
                };
                let path = self.substituted(path).replace('\\', "/");
                let text = (self.include)(&path).map_err(|reason| SfzError::Include {
                    path: path.clone(),
                    reason,
                })?;
                self.read(&text, depth + 1)?;
                after
            } else if let Some(after) = rest.strip_prefix('<') {
                let Some((header, after)) = after.split_once('>') else {
                    break;
                };
                self.builder.header(header.trim());
                after
            } else {
                match rest.split_once('=') {
                    // An opcode: its name, then its value up to what comes next.
                    Some((name, after)) if !name.contains(char::is_whitespace) => {
                        let end = value_end(after);
                        let name = self.substituted(name);
                        let value = self.substituted(after[..end].trim());
                        self.builder.opcode(&name, &value);
                        &after[end..]
                    }
                    // A stray word: skipped.
                    _ => word(rest).1,
                }
            };
            rest = rest.trim_start();
        }
        Ok(())
    }

    fn substituted(&self, text: &str) -> String {
        let mut text = text.to_string();
        if text.contains('$') {
            for (name, value) in &self.defines {
                text = text.replace(name.as_str(), value);
            }
        }
        text
    }
}

/// The next word of `text`, after spaces, and what follows it.
fn word(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    text.split_at(end)
}

/// `text` without `// line` and `/* block */` comments.
fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let line = rest.find("//");
        let block = rest.find("/*");
        match (line, block) {
            (Some(line), block) if block.is_none_or(|block| line < block) => {
                out.push_str(&rest[..line]);
                rest = &rest[line..];
                rest = rest.find('\n').map_or("", |end| &rest[end..]);
            }
            (_, Some(block)) => {
                out.push_str(&rest[..block]);
                rest = &rest[block + 2..];
                match rest.find("*/") {
                    Some(end) => {
                        // A block over several lines keeps its line breaks, so what follows
                        // stays on its own line.
                        out.extend(rest[..end].chars().filter(|&c| c == '\n'));
                        rest = &rest[end + 2..];
                    }
                    None => rest = "",
                }
            }
            _ => {
                out.push_str(rest);
                return out;
            }
        }
    }
}

/// Where the value at the start of `text` ends: at the end of its line, or before the next
/// `<header>`, ` name=` or ` #directive`. So a sample path may hold spaces:
/// `sample=Cello Section\a.wav lokey=40`.
fn value_end(text: &str) -> usize {
    let bytes = text.as_bytes();
    let is_name = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$';
    for (index, &byte) in bytes.iter().enumerate() {
        if byte == b'<' || byte == b'\n' || byte == b'\r' {
            return index;
        }
        if byte.is_ascii_whitespace() {
            if bytes.get(index + 1) == Some(&b'#') {
                return index;
            }
            let word = &bytes[index + 1..];
            let name_length = word.iter().take_while(|&&byte| is_name(byte)).count();
            if name_length > 0 && word.get(name_length) == Some(&b'=') {
                return index;
            }
        }
    }
    text.len()
}

/// The scope opcodes go into.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Scope {
    /// Before any header, and inside headers this does not read, such as `<curve>`.
    #[default]
    Ignored,
    Control,
    Global,
    Master,
    Group,
    Region,
}

/// Opcodes of one scope, the last of the same name winning.
type Opcodes = HashMap<String, String>;

#[derive(Default)]
struct Builder {
    scope: Scope,
    default_path: String,
    /// The value each controller has before it moves, from 0 to 1.
    controllers: HashMap<u32, f32>,
    global: Opcodes,
    master: Opcodes,
    group: Opcodes,
    region: Opcodes,
    sfz: Sfz,
}

impl Builder {
    fn opcode(&mut self, name: &str, value: &str) {
        let target = match self.scope {
            Scope::Ignored => return,
            Scope::Control => return self.control(name, value),
            Scope::Global => &mut self.global,
            Scope::Master => &mut self.master,
            Scope::Group => &mut self.group,
            Scope::Region => &mut self.region,
        };
        target.insert(name.to_ascii_lowercase(), value.to_string());
    }

    fn header(&mut self, header: &str) {
        if self.scope == Scope::Region {
            self.finish_region();
        }
        let scope = match header {
            "control" => Scope::Control,
            "global" => {
                self.global.clear();
                self.master.clear();
                self.group.clear();
                Scope::Global
            }
            "master" => {
                self.master.clear();
                self.group.clear();
                Scope::Master
            }
            "group" => {
                self.group.clear();
                Scope::Group
            }
            "region" => {
                self.region.clear();
                Scope::Region
            }
            _ => Scope::Ignored,
        };
        self.scope = scope;
    }

    fn control(&mut self, name: &str, value: &str) {
        let name = name.to_ascii_lowercase();
        if name == "default_path" {
            self.default_path = value.replace('\\', "/");
        } else if let Some(number) = name.strip_prefix("set_cc") {
            if let (Ok(number), Some(value)) = (number.parse(), number_of(value)) {
                self.controllers
                    .insert(number, (value / 127.0).clamp(0.0, 1.0));
            }
        } else if let Some(number) = name.strip_prefix("set_hdcc")
            && let (Ok(number), Some(value)) = (number.parse(), number_of(value))
        {
            self.controllers.insert(number, value.clamp(0.0, 1.0));
        }
    }

    fn finish_region(&mut self) {
        let mut opcodes = self.global.clone();
        opcodes.extend(self.master.iter().map(|(k, v)| (k.clone(), v.clone())));
        opcodes.extend(self.group.iter().map(|(k, v)| (k.clone(), v.clone())));
        opcodes.extend(self.region.drain());
        let opcodes = Resolved(opcodes);
        if self.sfz.switch_keys.is_none()
            && let (Some(low), Some(high)) = (opcodes.key("sw_lokey"), opcodes.key("sw_hikey"))
        {
            self.sfz.switch_keys = Some((low.min(high), low.max(high)));
        }
        if self.sfz.switch_default.is_none() {
            self.sfz.switch_default = opcodes.key("sw_default");
        }
        if let Some(region) = region(&opcodes, &self.default_path, &self.controllers) {
            self.sfz.regions.push(region);
        }
    }

    fn finish(mut self) -> Sfz {
        if self.scope == Scope::Region {
            self.finish_region();
        }
        self.sfz
    }
}

/// The opcodes of a region after inheritance.
struct Resolved(Opcodes);

impl Resolved {
    fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    /// The first of `names` that is there: an opcode and its older spellings.
    fn first(&self, names: &[&str]) -> Option<&str> {
        names.iter().find_map(|name| self.get(name))
    }

    fn number(&self, names: &[&str]) -> Option<f32> {
        self.first(names).and_then(number_of)
    }

    fn frames(&self, name: &str) -> Option<u64> {
        let value = self.get(name)?.trim().parse::<f64>().ok()?;
        (value >= 0.0).then_some(value as u64)
    }

    /// What the controllers of `name` add to it at their default values: the sum of each
    /// `<name>_onccN` or `<name>_ccN` times controller N, from 0 to 1.
    fn by_controllers(&self, name: &str, controllers: &HashMap<u32, f32>) -> f32 {
        let mut sum = 0.0;
        for (opcode, value) in &self.0 {
            let Some(rest) = opcode
                .strip_prefix(name)
                .and_then(|rest| rest.strip_prefix('_'))
            else {
                continue;
            };
            let number = rest.strip_prefix("oncc").or(rest.strip_prefix("cc"));
            if let (Some(Ok(number)), Some(value)) =
                (number.map(str::parse::<u32>), number_of(value))
            {
                sum += value * controllers.get(&number).copied().unwrap_or(0.0);
            }
        }
        sum
    }

    /// A key, as a number or a note name. -1 is "no key", which is `None` too.
    fn key(&self, name: &str) -> Option<u8> {
        self.get(name).and_then(key_of)
    }
}

fn number_of(value: &str) -> Option<f32> {
    value
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
}

/// A key from a number, `60`, or a note name, `c4`, `C#4`, `eb3`: C4 is 60, as SFZ counts.
fn key_of(value: &str) -> Option<u8> {
    let value = value.trim();
    if let Ok(number) = value.parse::<i32>() {
        return u8::try_from(number).ok().filter(|key| *key <= 127);
    }
    let mut characters = value.chars();
    let letter = characters.next()?.to_ascii_lowercase();
    let step = match letter {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    };
    let rest = characters.as_str();
    let (accidental, octave) = match rest.chars().next()? {
        '#' => (1, &rest[1..]),
        'b' if rest.len() > 1 => (-1, &rest[1..]),
        _ => (0, rest),
    };
    let octave: i32 = octave.parse().ok()?;
    let key = (octave + 1) * 12 + step + accidental;
    u8::try_from(key).ok().filter(|key| *key <= 127)
}

/// The region of resolved opcodes, or `None` for one that cannot play as written.
fn region(
    opcodes: &Resolved,
    default_path: &str,
    controllers: &HashMap<u32, f32>,
) -> Option<Region> {
    let sample = opcodes.get("sample")?.replace('\\', "/");
    // A generator such as `*sine` or `*silence`, not a file.
    if sample.starts_with('*') {
        return None;
    }
    let trigger = match opcodes.get("trigger").unwrap_or("attack") {
        "release" | "release_key" => Trigger::Release,
        "legato" => return None,
        _ => Trigger::Attack,
    };
    for (name, _) in &opcodes.0 {
        // Started by a controller rather than a key.
        if name.starts_with("on_locc") || name.starts_with("on_hicc") {
            return None;
        }
    }
    if !controllers_allow(opcodes, controllers) {
        return None;
    }
    let key = opcodes.key("key");
    let low_key = opcodes.key("lokey").or(key).unwrap_or(0);
    let high_key = match opcodes.get("hikey").or(opcodes.get("key")) {
        // `hikey=-1` turns the region off.
        Some(value) => key_of(value)?,
        None => 127,
    };
    if low_key > high_key {
        return None;
    }
    let keycenter = opcodes.key("pitch_keycenter").or(key).unwrap_or(60);
    let low_velocity = opcodes.number(&["lovel"]).map_or(1, velocity);
    let high_velocity = opcodes.number(&["hivel"]).map_or(127, velocity);
    let switch = match (opcodes.key("sw_lolast"), opcodes.key("sw_hilast")) {
        (Some(low), Some(high)) => Some((low.min(high), low.max(high))),
        _ => opcodes.key("sw_last").map(|key| (key, key)),
    };
    let loop_mode = opcodes
        .first(&["loop_mode", "loopmode"])
        .and_then(|mode| match mode {
            "no_loop" => Some(LoopMode::NoLoop),
            "one_shot" => Some(LoopMode::OneShot),
            "loop_continuous" => Some(LoopMode::Continuous),
            "loop_sustain" => Some(LoopMode::Sustain),
            _ => None,
        });
    let offset = opcodes.frames("offset").unwrap_or(0);
    let end = opcodes.frames("end");
    if end.is_some_and(|end| end <= offset) {
        return None;
    }
    let off_mode = match opcodes.get("off_mode") {
        Some("normal") => OffMode::Normal,
        Some("time") => OffMode::Time(opcodes.number(&["off_time"]).unwrap_or(0.006)),
        _ => OffMode::Fast,
    };
    // A number with what its controllers add at their defaults, such as a release that a
    // controller lengthens: `ampeg_release_oncc72=2` with `set_hdcc72=0.4` adds 0.8 s.
    let moved = |name: &str, default: f32| {
        opcodes.number(&[name]).unwrap_or(default) + opcodes.by_controllers(name, controllers)
    };
    let percent = |name: &str, default: f32| moved(name, default) / 100.0;
    Some(Region {
        sample: format!("{default_path}{sample}"),
        keys: (low_key, high_key),
        velocities: (low_velocity.min(high_velocity), high_velocity),
        keycenter,
        keytrack_cents: opcodes.number(&["pitch_keytrack"]).unwrap_or(100.0),
        tune_cents: opcodes.number(&["tune", "pitch"]).unwrap_or(0.0)
            + 100.0 * opcodes.number(&["transpose"]).unwrap_or(0.0),
        random: (
            opcodes.number(&["lorand"]).unwrap_or(0.0),
            opcodes.number(&["hirand"]).unwrap_or(1.0),
        ),
        sequence: (
            opcodes
                .number(&["seq_length"])
                .map_or(1, |n| n.max(1.0) as u32),
            opcodes
                .number(&["seq_position"])
                .map_or(1, |n| n.max(1.0) as u32),
        ),
        switch,
        trigger,
        offset,
        end,
        loop_mode,
        loop_start: opcodes
            .frames("loop_start")
            .or_else(|| opcodes.frames("loopstart")),
        loop_end: opcodes
            .frames("loop_end")
            .or_else(|| opcodes.frames("loopend")),
        // The volume of each level adds up.
        volume_db: moved("volume", 0.0)
            + ["group_volume", "master_volume", "global_volume"]
                .iter()
                .filter_map(|name| opcodes.number(&[name]))
                .sum::<f32>(),
        amplitude: opcodes.number(&["amplitude"]).unwrap_or(100.0).max(0.0) / 100.0,
        pan: opcodes.number(&["pan"]).unwrap_or(0.0).clamp(-100.0, 100.0) / 100.0,
        velocity_tracking: percent("amp_veltrack", 100.0).clamp(-1.0, 1.0),
        envelope: Adsr {
            attack: moved("ampeg_attack", 0.0).max(0.0),
            decay: moved("ampeg_decay", 0.0).max(0.0),
            sustain: percent("ampeg_sustain", 100.0).clamp(0.0, 1.0),
            release: moved("ampeg_release", 0.0).max(0.0),
        },
        group: opcodes
            .number(&["group", "polyphony_group"])
            .map_or(0, |group| group as i64),
        off_by: opcodes.number(&["off_by"]).map(|group| group as i64),
        off_mode,
    })
}

fn velocity(value: f32) -> u8 {
    value.clamp(0.0, 127.0) as u8
}

/// Whether every `loccN` and `hiccN` of a region holds for controller N at its default value.
/// Controllers do not move here, so a region for another position of one never plays.
fn controllers_allow(opcodes: &Resolved, controllers: &HashMap<u32, f32>) -> bool {
    opcodes.0.iter().all(|(name, value)| {
        let (bound, number) = if let Some(number) = name.strip_prefix("locc") {
            (Bound::Low, number)
        } else if let Some(number) = name.strip_prefix("hicc") {
            (Bound::High, number)
        } else {
            return true;
        };
        let (Ok(number), Some(limit)) = (number.parse::<u32>(), number_of(value)) else {
            return true;
        };
        let at = controllers.get(&number).copied().unwrap_or(0.0) * 127.0;
        match bound {
            Bound::Low => at >= limit,
            Bound::High => at <= limit,
        }
    })
}

enum Bound {
    Low,
    High,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_includes(path: &str) -> Result<String, String> {
        Err(format!("no file {path}"))
    }

    fn read(text: &str) -> Sfz {
        parse(text, &no_includes).unwrap()
    }

    #[test]
    fn note_names_count_c4_as_60() {
        assert_eq!(key_of("c4"), Some(60));
        assert_eq!(key_of("C#4"), Some(61));
        assert_eq!(key_of("eb3"), Some(51));
        assert_eq!(key_of("c-1"), Some(0));
        assert_eq!(key_of("g9"), Some(127));
        assert_eq!(key_of("b"), None);
        assert_eq!(key_of("-1"), None);
        assert_eq!(key_of("128"), None);
    }

    #[test]
    fn a_region_inherits_from_its_group_master_and_global_and_overrides_them() {
        let sfz = read(
            "<global> ampeg_release=0.8 volume=-3
             <master> lovel=1 hivel=62
             <group> lokey=40 hikey=45 pitch_keycenter=42
             <region> sample=a.wav
             <region> sample=b.wav volume=2 lovel=63 hivel=127",
        );
        let [a, b] = &sfz.regions[..] else {
            panic!("{sfz:?}")
        };
        assert_eq!((a.keys, a.keycenter, a.velocities), ((40, 45), 42, (1, 62)));
        assert_eq!((a.volume_db, a.envelope.release), (-3.0, 0.8));
        assert_eq!((b.volume_db, b.velocities), (2.0, (63, 127)));
    }

    /// The VSCO 2 layout: opcodes one per line, a default path with backslashes and a space.
    #[test]
    fn a_default_path_with_backslashes_and_spaces_comes_before_every_sample() {
        let sfz = read(
            "<control>\ndefault_path=Strings\\Cello Section\\susvib\\\n\n<group> //all\n\
             <region>\nsample=susvib_A2_v1_1.wav\nlokey=55\nhikey=58\npitch_keycenter=57\n",
        );
        assert_eq!(
            sfz.regions[0].sample,
            "Strings/Cello Section/susvib/susvib_A2_v1_1.wav"
        );
        assert_eq!(sfz.regions[0].keys, (55, 58));
    }

    #[test]
    fn a_sample_path_may_hold_spaces_and_ends_at_the_next_opcode() {
        let sfz = read("<region> sample=My Piano/C 4.wav key=c4 <region>sample=x.wav");
        assert_eq!(sfz.regions[0].sample, "My Piano/C 4.wav");
        assert_eq!(
            (sfz.regions[0].keys, sfz.regions[0].keycenter),
            ((60, 60), 60)
        );
        assert_eq!(sfz.regions[1].sample, "x.wav");
    }

    /// The Salamander and Karoryfer style: defines, includes and keyswitches in masters.
    #[test]
    fn defines_and_includes_are_put_in_place() {
        let files = |path: &str| match path {
            "Data/notes.txt" => Ok("<region> sample=$EXT_TEST.$EXT key=$NATURAL".to_string()),
            other => Err(format!("no file {other}")),
        };
        let sfz = parse(
            "#define $NATURAL 24\n#define $EXT flac\n#define $EXT_TEST piano\n\
             <global> sw_lokey=c0 sw_hikey=c#0 sw_default=c0\n\
             <master> sw_last=$NATURAL\n#include \"Data/notes.txt\"",
            &files,
        )
        .unwrap();
        assert_eq!(sfz.switch_keys, Some((12, 13)));
        assert_eq!(sfz.switch_default, Some(12));
        let region = &sfz.regions[0];
        assert_eq!(region.sample, "piano.flac");
        assert_eq!((region.keys, region.switch), ((24, 24), Some((24, 24))));
    }

    #[test]
    fn an_include_that_does_not_read_says_which_and_one_that_includes_itself_stops() {
        let error = parse("#include \"x.sfz\"", &no_includes).unwrap_err();
        assert_eq!(
            error.to_string(),
            "it includes x.sfz, which cannot be read: no file x.sfz"
        );
        let itself = |_: &str| Ok("#include \"self.sfz\"".to_string());
        assert_eq!(
            parse("#include \"self.sfz\"", &itself),
            Err(SfzError::TooDeep)
        );
    }

    #[test]
    fn comments_go_wherever_they_are() {
        let sfz = read(
            "/* a block\n over lines */ <region> sample=a.wav // the a\n<region> /* x */ sample=b.wav",
        );
        let samples: Vec<_> = sfz.regions.iter().map(|r| r.sample.as_str()).collect();
        assert_eq!(samples, ["a.wav", "b.wav"]);
    }

    #[test]
    fn regions_that_cannot_play_as_written_are_left_out() {
        let sfz = read(
            "<control> set_cc64=0 set_hdcc20=1
             <region> sample=kept.wav locc20=100
             <region> sample=pedal-down.wav locc64=64
             <region> sample=cc-started.wav on_locc64=64
             <region> sample=legato.wav trigger=legato
             <region> sample=*sine
             <region> sample=off.wav hikey=-1",
        );
        let samples: Vec<_> = sfz.regions.iter().map(|r| r.sample.as_str()).collect();
        assert_eq!(samples, ["kept.wav"]);
    }

    /// The Headroom piano: directives in the middle of a line, and a define that changes
    /// between the regions that include the same file.
    #[test]
    fn directives_may_come_anywhere_on_a_line() {
        let files = |path: &str| match path {
            "sample.txt" => Ok("sample=PIANO $MIC $KEY.flac pitch_keycenter=$KEY".to_string()),
            other => Err(format!("no file {other}")),
        };
        let sfz = parse(
            "#define $MIC CLOSE\n<group> lovel=1 hivel=59 group_volume=13.3 volume=-1\n\
             <region> #define $KEY 21 lokey=21 hikey=22 #include \"sample.txt\"\n\
             <region> #define $KEY 24 lokey=23 hikey=25 #include \"sample.txt\"",
            &files,
        )
        .unwrap();
        let [low, high] = &sfz.regions[..] else {
            panic!("{sfz:?}")
        };
        assert_eq!(
            (low.sample.as_str(), low.keys, low.keycenter),
            ("PIANO CLOSE 21.flac", (21, 22), 21)
        );
        assert_eq!(
            (high.sample.as_str(), high.keys, high.keycenter),
            ("PIANO CLOSE 24.flac", (23, 25), 24)
        );
        assert_eq!(low.volume_db, 12.3);
    }

    /// The controllers do not move here, so what they add at their defaults is part of the
    /// value: Salamander lowers its velocity tracking this way, Headroom lengthens its release.
    #[test]
    fn controllers_add_what_they_add_at_their_defaults() {
        let sfz = read(
            "<control> set_hdcc99=0.73 set_hdcc72=0.4
             <global> amp_veltrack_oncc99=-100 ampeg_release=0.1 ampeg_release_oncc72=2
             <region> sample=a.wav",
        );
        let region = &sfz.regions[0];
        assert!((region.velocity_tracking - 0.27).abs() < 1e-6, "{region:?}");
        assert!((region.envelope.release - 0.9).abs() < 1e-6, "{region:?}");
    }

    #[test]
    fn unknown_opcodes_and_unreadable_values_are_ignored() {
        let sfz = read("<curve> v000=0 <region> sample=a.wav bend_up=1200 volume=loud hivel=90");
        let region = &sfz.regions[0];
        assert_eq!((region.volume_db, region.velocities), (0.0, (1, 90)));
    }

    #[test]
    fn round_robin_release_loop_and_choke_opcodes_are_read() {
        let sfz = read(
            "<region> sample=a.wav seq_length=3 seq_position=2 lorand=0.25 hirand=0.5
               trigger=release loopmode=loop_sustain loopstart=10 loopend=99
               group=1 off_by=2 off_mode=time off_time=0.1 transpose=-12 tune=5",
        );
        let region = &sfz.regions[0];
        assert_eq!(region.sequence, (3, 2));
        assert_eq!(region.random, (0.25, 0.5));
        assert_eq!(region.trigger, Trigger::Release);
        assert_eq!(region.loop_mode, Some(LoopMode::Sustain));
        assert_eq!((region.loop_start, region.loop_end), (Some(10), Some(99)));
        assert_eq!(
            (region.group, region.off_by, region.off_mode),
            (1, Some(2), OffMode::Time(0.1))
        );
        assert_eq!(region.tune_cents, -1195.0);
    }
}
