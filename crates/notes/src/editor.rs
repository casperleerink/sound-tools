//! Clip editors: what the window shows a clip in. An editor is a description, its rows and its
//! fields, made once when it loads; every gesture runs in the one note editor of the window.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;

use serde::Deserialize;

use crate::view::{EditorName, FieldName, View, ViewValue};
use crate::{Clip, Pitch};

/// The most clip choices an editor may combine: each combination is a grid made at load.
const MAX_COMBINATIONS: usize = 256;
const MAX_LABEL: usize = 24;

/// The rows an editor shows for one combination of its clip choices.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Grid {
    /// Lowest pitch first.
    pub rows: Vec<Row>,
    #[serde(default = "default_row_height")]
    pub row_height: f32,
    #[serde(default)]
    pub notes: NoteShape,
}

fn default_row_height() -> f32 {
    12.0
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub pitch: Pitch,
    #[serde(default)]
    pub label: Option<String>,
    /// A darker row, such as a black key.
    #[serde(default)]
    pub shade: bool,
}

/// How a note is drawn and dragged.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteShape {
    /// As long as the note, with an end to drag.
    #[default]
    Bar,
    /// A square at its start, drawn one snap step long and never resized: a drum hit.
    Hit,
}

impl Grid {
    pub fn check(&self) -> Result<(), String> {
        let count = self.rows.len();
        if !(1..=128).contains(&count) {
            return Err(format!("a grid has 1 to 128 rows, not {count}"));
        }
        if !(8.0..=40.0).contains(&self.row_height) {
            return Err(format!(
                "rowHeight must be from 8 to 40, not {}",
                self.row_height
            ));
        }
        for (index, pair) in self.rows.windows(2).enumerate() {
            if let [lower, higher] = pair
                && higher.pitch <= lower.pitch
            {
                return Err(format!(
                    "rows[{}].pitch must be higher than {}, the pitch of the row before it: rows go from the lowest pitch up",
                    index + 1,
                    lower.pitch.number()
                ));
            }
        }
        let labels = self.rows.iter().enumerate();
        for (index, row) in labels {
            let length = row.label.as_ref().map_or(0, |label| label.chars().count());
            if length > MAX_LABEL {
                return Err(format!(
                    "rows[{index}].label must be at most {MAX_LABEL} characters, not {length}"
                ));
            }
        }
        Ok(())
    }
}

/// A field an editor keeps in a `view`: the same JSON as `choice()` and `toggle()` of the SDK.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewField {
    Choice {
        options: Vec<ViewValue>,
        default: ViewValue,
        #[serde(default)]
        label: Option<String>,
    },
    Toggle {
        default: bool,
        #[serde(default)]
        label: Option<String>,
    },
}

impl ViewField {
    /// Every value it takes.
    pub fn options(&self) -> Vec<ViewValue> {
        match self {
            Self::Choice { options, .. } => options.clone(),
            Self::Toggle { .. } => vec![ViewValue::Toggle(false), ViewValue::Toggle(true)],
        }
    }

    pub fn default_value(&self) -> ViewValue {
        match self {
            Self::Choice { default, .. } => default.clone(),
            Self::Toggle { default, .. } => ViewValue::Toggle(*default),
        }
    }

    pub fn accepts(&self, value: &ViewValue) -> bool {
        match self {
            Self::Choice { options, .. } => options.contains(value),
            Self::Toggle { .. } => matches!(value, ViewValue::Toggle(_)),
        }
    }

    fn check(&self) -> Result<(), String> {
        let Self::Choice {
            options, default, ..
        } = self
        else {
            return Ok(());
        };
        if options.is_empty() {
            return Err("has no options".to_owned());
        }
        if options.iter().collect::<BTreeSet<_>>().len() != options.len() {
            return Err("has an option twice".to_owned());
        }
        if !options.contains(default) {
            return Err(format!("has the default {default}, which is not an option"));
        }
        Ok(())
    }

    /// Its options as a message lists them.
    fn listed(&self) -> String {
        match self {
            Self::Choice { options, .. } => {
                let options: Vec<String> = options.iter().map(ToString::to_string).collect();
                format!("one of {}", options.join(", "))
            }
            Self::Toggle { .. } => "true or false".to_owned(),
        }
    }
}

/// A clip editor: its fields, and its grid for every combination of its clip choices, made
/// once when it loads.
#[derive(Clone, Debug)]
pub struct EditorSpec {
    pub name: EditorName,
    pub title: String,
    /// The file under `extensions/` that defines it. `None` for a built-in editor.
    pub file: Option<String>,
    clip_fields: Vec<(FieldName, ViewField)>,
    note_fields: Vec<(FieldName, ViewField)>,
    /// By the value of every clip field.
    grids: BTreeMap<View, Arc<Grid>>,
    /// The grid of the defaults.
    default_grid: Arc<Grid>,
}

impl EditorSpec {
    /// Refuses fields that do not check, a grid that does not check, and a missing or extra
    /// combination of clip choices.
    pub fn new(
        name: EditorName,
        title: String,
        file: Option<String>,
        clip_fields: Vec<(FieldName, ViewField)>,
        note_fields: Vec<(FieldName, ViewField)>,
        grids: impl IntoIterator<Item = (View, Grid)>,
    ) -> Result<Self, String> {
        for (kind, fields) in [("clip", &clip_fields), ("note", &note_fields)] {
            let mut seen = BTreeSet::new();
            for (field, view_field) in fields {
                if !seen.insert(field) {
                    return Err(format!(
                        "the {kind} field {} is there twice",
                        field.as_str()
                    ));
                }
                view_field
                    .check()
                    .map_err(|problem| format!("the {kind} field {} {problem}", field.as_str()))?;
            }
        }
        let combinations = combinations(&clip_fields)?;
        let mut made = BTreeMap::new();
        for (choices, grid) in grids {
            if !combinations.contains(&choices) {
                return Err(format!(
                    "it has a grid for the choices {choices}, which its clip fields do not make"
                ));
            }
            grid.check().map_err(|problem| {
                format!("its grid with the choices {choices} fails: {problem}")
            })?;
            made.insert(choices, Arc::new(grid));
        }
        if let Some(missing) = combinations
            .iter()
            .find(|choices| !made.contains_key(*choices))
        {
            return Err(format!("it has no grid for the choices {missing}"));
        }
        let defaults = chosen(&clip_fields, &View::default());
        let Some(default_grid) = made.get(&defaults).cloned() else {
            return Err(format!("it has no grid for the choices {defaults}"));
        };
        Ok(Self {
            name,
            title,
            file,
            clip_fields,
            note_fields,
            grids: made,
            default_grid,
        })
    }

    pub fn clip_fields(&self) -> &[(FieldName, ViewField)] {
        &self.clip_fields
    }

    pub fn note_fields(&self) -> &[(FieldName, ViewField)] {
        &self.note_fields
    }

    /// The grid for the view of a clip. A field it lacks, or whose value is not an option, is
    /// at its default.
    pub fn grid_for(&self, clip_view: &View) -> Arc<Grid> {
        let choices = chosen(&self.clip_fields, clip_view);
        let grid = self.grids.get(&choices);
        grid.cloned().unwrap_or_else(|| self.default_grid.clone())
    }
}

/// The value of every field in `view`, or its default.
fn chosen(fields: &[(FieldName, ViewField)], view: &View) -> View {
    let mut choices = View::default();
    for (field, view_field) in fields {
        let value = view
            .get(field.as_str())
            .filter(|value| view_field.accepts(value));
        let value = value.cloned().unwrap_or_else(|| view_field.default_value());
        choices.insert(field.clone(), value);
    }
    choices
}

/// Every combination of the options of the fields.
fn combinations(fields: &[(FieldName, ViewField)]) -> Result<BTreeSet<View>, String> {
    let mut made = vec![View::default()];
    for (field, view_field) in fields {
        let options = view_field.options();
        if made.len() * options.len() > MAX_COMBINATIONS {
            return Err(format!(
                "its clip fields make more than {MAX_COMBINATIONS} combinations of choices"
            ));
        }
        made = made
            .iter()
            .flat_map(|view| {
                options.iter().map(|option| {
                    let mut view = view.clone();
                    view.insert(field.clone(), option.clone());
                    view
                })
            })
            .collect();
    }
    Ok(made.into_iter().collect())
}

/// The piano roll: a row per pitch, the black keys shaded and the Cs named, `C4` for 60.
pub fn piano_roll() -> EditorSpec {
    let rows = (0..=127).map(|number| {
        let pitch = Pitch::nearest(number);
        Row {
            pitch,
            label: (number % 12 == 0).then(|| pitch.name()),
            shade: matches!(number % 12, 1 | 3 | 6 | 8 | 10),
        }
    });
    let grid = Arc::new(Grid {
        rows: rows.collect(),
        row_height: default_row_height(),
        notes: NoteShape::Bar,
    });
    EditorSpec {
        name: EditorName::piano_roll(),
        title: "Piano roll".to_owned(),
        file: None,
        clip_fields: Vec::new(),
        note_fields: Vec::new(),
        grids: BTreeMap::from([(View::default(), grid.clone())]),
        default_grid: grid,
    }
}

/// The editors a project has: the piano roll and the editors of the project. Cheap to clone:
/// every copy is the same catalog. It lives on the thread of the project, like the project.
#[derive(Clone, Default)]
pub struct Editors(Rc<RefCell<Catalog>>);

struct Catalog {
    piano_roll: Arc<EditorSpec>,
    project: Vec<Arc<EditorSpec>>,
    /// Counts the changes, so a view knows when what it took from here is old.
    generation: u64,
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            piano_roll: Arc::new(piano_roll()),
            project: Vec::new(),
            generation: 0,
        }
    }
}

impl Catalog {
    fn all(&self) -> impl Iterator<Item = &Arc<EditorSpec>> {
        std::iter::once(&self.piano_roll).chain(&self.project)
    }

    fn find(&self, name: &EditorName) -> Option<&Arc<EditorSpec>> {
        self.all().find(|spec| spec.name == *name)
    }
}

/// What the window shows a clip in.
pub struct Shown {
    pub spec: Arc<EditorSpec>,
    pub grid: Arc<Grid>,
    /// The editor the clip names when this project does not have it: the clip shows in the
    /// piano roll.
    pub missing: Option<EditorName>,
}

impl Editors {
    /// Replaces the editors of the project. Gives the name of each refused one and why: the
    /// name of the piano roll, or a name another one has.
    pub fn set_project(&self, specs: Vec<EditorSpec>) -> Vec<(String, String)> {
        let mut catalog = self.0.borrow_mut();
        let mut refused = Vec::new();
        let mut kept: Vec<Arc<EditorSpec>> = Vec::new();
        for spec in specs {
            let name = spec.name.as_str().to_owned();
            if spec.name == catalog.piano_roll.name {
                refused.push((name, "it is the name of a built-in editor".to_owned()));
            } else if kept.iter().any(|other| other.name == spec.name) {
                refused.push((
                    name,
                    "another editor of the project has this name".to_owned(),
                ));
            } else {
                kept.push(Arc::new(spec));
            }
        }
        catalog.project = kept;
        catalog.generation += 1;
        refused
    }

    pub fn all(&self) -> Vec<Arc<EditorSpec>> {
        self.0.borrow().all().cloned().collect()
    }

    /// The editors of the project: all but the piano roll.
    pub fn project(&self) -> Vec<Arc<EditorSpec>> {
        self.0.borrow().project.clone()
    }

    pub fn generation(&self) -> u64 {
        self.0.borrow().generation
    }

    pub fn of(&self, clip: &Clip) -> Shown {
        let catalog = self.0.borrow();
        let found = clip.editor.as_ref().map(|name| (name, catalog.find(name)));
        let (spec, missing) = match found {
            Some((_, Some(spec))) => (spec.clone(), None),
            Some((name, None)) => (catalog.piano_roll.clone(), Some(name.clone())),
            None => (catalog.piano_roll.clone(), None),
        };
        Shown {
            grid: spec.grid_for(&clip.view),
            spec,
            missing,
        }
    }

    /// What is wrong with the editor or a view of a clip, the first thing only, written to
    /// follow the name of its file. The values of the fields of its own editor are checked; a
    /// field no editor of the project has is a typo; a field of another editor is left alone.
    pub fn problem(&self, clip: &Clip) -> Option<String> {
        let plain = clip.editor.is_none()
            && clip.view.is_empty()
            && clip.notes.iter().all(|note| note.view.is_empty());
        if plain {
            return None;
        }
        let catalog = self.0.borrow();
        let spec = match &clip.editor {
            None => &catalog.piano_roll,
            Some(name) => match catalog.find(name) {
                Some(spec) => spec,
                None => {
                    let names: Vec<&str> = catalog.all().map(|spec| spec.name.as_str()).collect();
                    return Some(format!(
                        "names the clip editor {:?}, which this project does not have, so the window shows it in the piano roll and keeps its editor and view. The editors: {}",
                        name.as_str(),
                        names.join(", ")
                    ));
                }
            },
        };
        let clip_problem = clip.view.iter().find_map(|(field, value)| {
            field_problem(&catalog, spec, Kind::Clip, field, value, "view")
        });
        clip_problem.or_else(|| {
            clip.notes.iter().enumerate().find_map(|(index, note)| {
                let path = format!("notes[{index}].view");
                note.view.iter().find_map(|(field, value)| {
                    field_problem(&catalog, spec, Kind::Note, field, value, &path)
                })
            })
        })
    }
}

#[derive(Copy, Clone)]
enum Kind {
    Clip,
    Note,
}

impl Kind {
    fn fields(self, spec: &EditorSpec) -> &[(FieldName, ViewField)] {
        match self {
            Self::Clip => spec.clip_fields(),
            Self::Note => spec.note_fields(),
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Clip => "clip",
            Self::Note => "note",
        }
    }
}

/// The problem of one field of a view at `path`, or `None`.
fn field_problem(
    catalog: &Catalog,
    spec: &EditorSpec,
    kind: Kind,
    field: &FieldName,
    value: &ViewValue,
    path: &str,
) -> Option<String> {
    fn declared<'a>(spec: &'a EditorSpec, kind: Kind, field: &FieldName) -> Option<&'a ViewField> {
        let mut fields = kind.fields(spec).iter();
        fields
            .find(|(name, _)| name == field)
            .map(|(_, view_field)| view_field)
    }
    match declared(spec, kind, field) {
        Some(view_field) if view_field.accepts(value) => None,
        Some(view_field) => Some(format!(
            "has {value} in {path}.{}, which the editor {} does not take, so it shows as {}. Use {}",
            field.as_str(),
            spec.name.as_str(),
            view_field.default_value(),
            view_field.listed()
        )),
        None if catalog
            .all()
            .any(|other| declared(other, kind, field).is_some()) =>
        {
            None
        }
        None => Some(format!(
            "has {path}.{}, which no editor of this project has as a {} field, so nothing reads it",
            field.as_str(),
            kind.word()
        )),
    }
}
