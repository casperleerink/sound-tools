//! The one keymap of the window: the actions of the app and of the tools of the project, and
//! the keys that do them. An action has default keys; the composer's own keys for it replace
//! them, and are kept in a file outside every project.
//!
//! An action is named `<group>.<action>` everywhere, such as `song.record`: in the file, and
//! where a button names the action whose key it shows.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{fmt, fs, io};

use gpui::{Global, Keystroke};

use crate::global_keys::chord_name;

/// Something a tool or the app can do, which a key does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub name: String,
    /// What the composer reads for it: "Play or stop".
    pub title: String,
    /// The keys that do it until the composer picks others, each as [`chord`] names it.
    pub keys: Vec<String>,
}

/// The actions of one tool, or of the app, in the order they are tried on a key they share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub title: String,
    pub actions: Vec<Action>,
}

/// The composer's own keys by action id. An empty list is an action with no key.
type OwnKeys = BTreeMap<String, Vec<String>>;

#[derive(Debug)]
pub enum ShortcutsError {
    Read { path: PathBuf, error: String },
    Write { path: PathBuf, error: io::Error },
}

impl fmt::Display for ShortcutsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, error } => write!(
                formatter,
                "the shortcuts in {} could not be read: {error}",
                path.display()
            ),
            Self::Write { path, error } => write!(
                formatter,
                "the shortcuts could not be kept in {}: {error}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ShortcutsError {}

#[derive(Default)]
pub struct Shortcuts {
    /// The groups that stay while the tools load again, such as the app's.
    fixed: Vec<Group>,
    tools: Vec<Group>,
    own: OwnKeys,
    /// Where the composer's own keys are kept. Without one they last as long as the window.
    file: Option<PathBuf>,
}

impl Global for Shortcuts {}

/// The id of the action `action` of `group`.
pub fn id(group: &str, action: &str) -> String {
    format!("{group}.{action}")
}

/// A key as an action names it: `a`, or with cmd, ctrl, alt and shift held first, in that order,
/// `cmd-shift-z`.
pub fn chord(keystroke: &Keystroke) -> String {
    let name = chord_name(keystroke);
    let key = keystroke.key.as_str();
    match (name.strip_suffix(key)).filter(|_| keystroke.modifiers.shift) {
        Some(held) => format!("{held}shift-{key}"),
        None => name,
    }
}

/// A chord as [`Kbd`](crate::components::kbd::Kbd) and a tooltip draw it: `cmd-alt-c` is
/// `mod+alt+c`.
pub fn drawn(mut chord: &str) -> String {
    let mut parts = Vec::new();
    for (prefix, drawn) in [
        ("cmd-", "mod"),
        ("ctrl-", "ctrl"),
        ("alt-", "alt"),
        ("shift-", "shift"),
    ] {
        if let Some(rest) = chord.strip_prefix(prefix) {
            parts.push(drawn);
            chord = rest;
        }
    }
    parts.push(chord);
    parts.join("+")
}

impl Shortcuts {
    /// The keymap with the composer's own keys of `file`, where each change of them is kept. A
    /// file that is not there yet holds none. One that does not read is an error, and stays as
    /// it is.
    pub fn open(file: PathBuf) -> Result<Self, ShortcutsError> {
        Ok(Self {
            own: read(&file)?,
            file: Some(file),
            ..Self::default()
        })
    }

    /// The tools of the project loaded: their groups replace those of the load before.
    pub fn set_tool_groups(&mut self, groups: Vec<Group>) {
        self.tools = groups;
    }

    /// Sets a group that stays while the tools load again, in place of one of its name.
    pub fn set_group(&mut self, group: Group) {
        match (self.fixed.iter_mut()).find(|fixed| fixed.name == group.name) {
            Some(fixed) => *fixed = group,
            None => self.fixed.push(group),
        }
    }

    /// Every group: those that stay first, then the tools'.
    pub fn groups(&self) -> impl Iterator<Item = &Group> {
        self.fixed.iter().chain(&self.tools)
    }

    fn group(&self, name: &str) -> Option<&Group> {
        self.groups().find(|group| group.name == name)
    }

    fn action(&self, id: &str) -> Option<&Action> {
        let (group, name) = id.rsplit_once('.')?;
        let actions = &self.group(group)?.actions;
        actions.iter().find(|action| action.name == name)
    }

    /// The keys that do the action `id` now: the composer's own, else its default ones.
    pub fn keys(&self, id: &str) -> &[String] {
        match (self.own.get(id), self.action(id)) {
            (Some(own), _) => own,
            (None, Some(action)) => &action.keys,
            (None, None) => &[],
        }
    }

    /// The actions of `group` that `chord` does, by name, in the order the group has them.
    pub fn bound<'a>(&'a self, group: &str, chord: &str) -> Vec<&'a str> {
        let Some(found) = self.group(group) else {
            return Vec::new();
        };
        (found.actions.iter())
            .filter(|action| self.does(&found.name, action, chord))
            .map(|action| action.name.as_str())
            .collect()
    }

    /// Every chord that does an action of `group`.
    pub fn chords<'a>(&'a self, group: &str) -> impl Iterator<Item = &'a str> {
        let group = self.group(group);
        (group.into_iter())
            .flat_map(|group| group.actions.iter().map(|action| (&group.name, action)))
            .flat_map(|(group, action)| self.keys(&id(group, &action.name)))
            .map(String::as_str)
    }

    /// The other actions `chord` does, each with its group, for a hint beside the action `id`
    /// that it shares its key.
    pub fn others_with<'a>(&'a self, id: &str, chord: &str) -> Vec<(&'a Group, &'a Action)> {
        (self.groups())
            .flat_map(|group| group.actions.iter().map(move |action| (group, action)))
            .filter(|(group, action)| self::id(&group.name, &action.name) != id)
            .filter(|(group, action)| self.does(&group.name, action, chord))
            .collect()
    }

    fn does(&self, group: &str, action: &Action, chord: &str) -> bool {
        (self.keys(&id(group, &action.name)).iter()).any(|key| key == chord)
    }

    /// Whether the composer's own keys for the action `id` are other than its default ones.
    pub fn is_changed(&self, id: &str) -> bool {
        let default = self.action(id).map(|action| &action.keys);
        (self.own.get(id)).is_some_and(|own| Some(own) != default)
    }

    /// The composer picks `keys` for the action `id`, none for an action with no key. Its
    /// default keys are the same as [`Shortcuts::reset`].
    pub fn set_keys(&mut self, id: &str, keys: Vec<String>) -> Result<(), ShortcutsError> {
        let default = self.action(id).is_some_and(|action| action.keys == keys);
        self.change(id, (!default).then_some(keys))
    }

    /// The action `id` has its default keys again.
    pub fn reset(&mut self, id: &str) -> Result<(), ShortcutsError> {
        self.change(id, None)
    }

    /// Changes the composer's own keys of one action, on the file as it is now: another window
    /// may have changed another action since this one read it. A file that does not read is an
    /// error and nothing changes, so it is never written over.
    fn change(&mut self, id: &str, keys: Option<Vec<String>>) -> Result<(), ShortcutsError> {
        let mut own = match &self.file {
            Some(file) => read(file)?,
            None => self.own.clone(),
        };
        match keys {
            Some(keys) => own.insert(id.to_string(), keys),
            None => own.remove(id),
        };
        if let Some(file) = &self.file {
            write(file, &own).map_err(|error| ShortcutsError::Write {
                path: file.clone(),
                error,
            })?;
        }
        self.own = own;
        Ok(())
    }
}

fn read(file: &Path) -> Result<OwnKeys, ShortcutsError> {
    let unread = |error: String| ShortcutsError::Read {
        path: file.to_path_buf(),
        error,
    };
    match fs::read_to_string(file) {
        Ok(text) => serde_json::from_str(&text).map_err(|error| unread(error.to_string())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(OwnKeys::new()),
        Err(error) => Err(unread(error.to_string())),
    }
}

/// Through a file of its own and a rename, so a crash never leaves half a file.
fn write(file: &Path, own: &OwnKeys) -> io::Result<()> {
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder)?;
    }
    let mut temporary = file.as_os_str().to_owned();
    temporary.push(format!(".{}.tmp", std::process::id()));
    fs::write(&temporary, serde_json::to_string_pretty(own)?)?;
    fs::rename(&temporary, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(name: &str, keys: &[&str]) -> Action {
        Action {
            name: name.to_string(),
            title: name.to_string(),
            keys: keys.iter().map(ToString::to_string).collect(),
        }
    }

    /// A song whose delete does three things by what is chosen, and plays on space.
    fn song() -> Group {
        Group {
            name: "song".to_string(),
            title: "Song".to_string(),
            actions: vec![
                action("play", &["space"]),
                action("delete-notes", &["delete", "backspace"]),
                action("delete-mark", &["delete"]),
                action("delete-clips", &["delete"]),
            ],
        }
    }

    fn keys(keys: &[&str]) -> Vec<String> {
        keys.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn the_composers_keys_replace_the_default_ones_until_a_reset() {
        let mut shortcuts = Shortcuts::default();
        shortcuts.set_tool_groups(vec![song()]);
        assert_eq!(shortcuts.bound("song", "space"), ["play"]);
        assert!(!shortcuts.is_changed("song.play"));

        shortcuts.set_keys("song.play", keys(&["p", "f5"])).unwrap();
        assert_eq!(shortcuts.keys("song.play"), ["p", "f5"]);
        assert_eq!(shortcuts.bound("song", "p"), ["play"]);
        assert!(shortcuts.bound("song", "space").is_empty());
        assert!(shortcuts.is_changed("song.play"));

        // No key at all is a choice too, and not the default.
        shortcuts.set_keys("song.play", Vec::new()).unwrap();
        assert!(shortcuts.keys("song.play").is_empty());
        assert!(shortcuts.bound("song", "p").is_empty());
        assert!(shortcuts.is_changed("song.play"));

        shortcuts.reset("song.play").unwrap();
        assert_eq!(shortcuts.bound("song", "space"), ["play"]);
        assert!(!shortcuts.is_changed("song.play"));
    }

    #[test]
    fn the_actions_of_one_key_come_in_the_order_their_group_has_them() {
        let mut shortcuts = Shortcuts::default();
        shortcuts.set_tool_groups(vec![song()]);
        let all = ["delete-notes", "delete-mark", "delete-clips"];
        assert_eq!(shortcuts.bound("song", "delete"), all);
        assert_eq!(shortcuts.bound("song", "backspace"), ["delete-notes"]);
        // A key the composer gives the last one does not move it first.
        shortcuts
            .set_keys("song.delete-clips", keys(&["backspace"]))
            .unwrap();
        assert_eq!(
            shortcuts.bound("song", "backspace"),
            ["delete-notes", "delete-clips"]
        );
        let others = shortcuts.others_with("song.delete-clips", "backspace");
        let others: Vec<&str> = (others.iter())
            .map(|(_, action)| action.name.as_str())
            .collect();
        assert_eq!(others, ["delete-notes"]);
    }

    #[test]
    fn the_tools_load_again_and_the_app_group_stays() {
        let mut shortcuts = Shortcuts::default();
        let app = Group {
            name: "app".to_string(),
            title: "Sound Tools".to_string(),
            actions: vec![action("undo", &["cmd-z"])],
        };
        shortcuts.set_group(app.clone());
        shortcuts.set_tool_groups(vec![song()]);
        shortcuts.set_tool_groups(Vec::new());
        assert_eq!(shortcuts.groups().collect::<Vec<_>>(), [&app]);
        assert!(shortcuts.bound("song", "space").is_empty());
    }

    #[test]
    fn a_change_keeps_the_keys_of_tools_that_are_not_loaded_and_of_another_window() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("support/shortcuts.json");
        let mut first = Shortcuts::open(file.clone()).unwrap();
        first.set_tool_groups(vec![song()]);
        first.set_keys("beat.tap", keys(&["t"])).unwrap();

        // A second window read the file before the first changed it again.
        let mut second = Shortcuts::open(file.clone()).unwrap();
        second.set_tool_groups(vec![song()]);
        first.set_keys("song.play", keys(&["p"])).unwrap();
        second
            .set_keys("song.delete-mark", keys(&["cmd-delete"]))
            .unwrap();

        let reopened = Shortcuts::open(file).unwrap();
        assert_eq!(reopened.keys("beat.tap"), ["t"]);
        assert_eq!(reopened.keys("song.play"), ["p"]);
        assert_eq!(reopened.keys("song.delete-mark"), ["cmd-delete"]);
        // The second window has the first one's change from then on.
        assert_eq!(second.keys("song.play"), ["p"]);
    }

    #[test]
    fn a_file_that_does_not_read_is_an_error_and_is_never_written_over() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("shortcuts.json");
        let mut shortcuts = Shortcuts::open(file.clone()).unwrap();
        shortcuts.set_tool_groups(vec![song()]);
        fs::write(&file, "{ \"song.play\": [\"p\"").unwrap();

        assert!(Shortcuts::open(file.clone()).is_err());
        assert!(shortcuts.set_keys("song.play", keys(&["o"])).is_err());
        assert_eq!(shortcuts.keys("song.play"), ["space"]);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "{ \"song.play\": [\"p\""
        );
    }

    #[test]
    fn a_chord_names_shift_after_the_other_modifiers() {
        let chord_of = |keys: &str| chord(&Keystroke::parse(keys).unwrap());
        assert_eq!(chord_of("space"), "space");
        assert_eq!(chord_of("shift-up"), "shift-up");
        assert_eq!(chord_of("shift-cmd-z"), "cmd-shift-z");
        assert_eq!(chord_of("alt-shift-ctrl-left"), "ctrl-alt-shift-left");
        assert_eq!(drawn("cmd-shift-z"), "mod+shift+z");
    }
}
