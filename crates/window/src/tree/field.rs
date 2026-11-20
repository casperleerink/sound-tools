//! A line of text the composer types: what it says is the window's while it has the keys, and
//! its code hears it once, when the keys leave it.

use std::collections::{BTreeSet, HashMap};

use rune::{Align, El, Key, KeyEvent, TextEdit, Tx, text};
use serde_json::{Value, json};
use sound_core::InstanceId;
use sound_typescript::window::Tools;

use crate::app::State;

use super::semantics::Press;
use super::{BoxNode, FieldNode, Node, Opens, Scope, Within};

/// The fields shown, by key in the window.
#[derive(Default)]
pub(crate) struct Fields {
    by_key: HashMap<String, Typed>,
    /// What the fields shown were last found in: the views' runs, the loads of the code, the
    /// overlays open and the instances shown.
    found_in: Option<(u64, u64, Vec<String>, BTreeSet<InstanceId>)>,
    /// The keys of the fields shown then.
    found: BTreeSet<String>,
    /// The key of the element the composer pressed last: where the keys go back to from a field
    /// that took them as it appeared.
    pub(crate) pressed: Option<String>,
    /// The items an open menu or list shows as its search field filters them, by the key of
    /// what opened it: what the arrows go to and Enter picks.
    found_items: HashMap<String, Vec<(String, Press)>>,
}

struct Typed {
    edit: TextEdit,
    /// The value its text was made from.
    from: String,
    press: Press,
    /// It had the keys at the last look, so their leaving commits what it says.
    had_keys: bool,
    back: Option<String>,
}

impl Typed {
    /// It is the search field of a menu or list: what it says only filters its items.
    fn searches(&self) -> bool {
        self.press
            .within
            .as_ref()
            .is_some_and(|within| within.picks)
    }

    fn new(value: &str, press: Press) -> Self {
        let mut edit = TextEdit::new(value);
        (edit.anchor, edit.focus) = (0, edit.text.len());
        Self {
            edit,
            from: value.to_string(),
            press,
            had_keys: false,
            back: None,
        }
    }
}

/// A field as a view draws it.
pub(super) fn typed(scope: &Scope, f: &FieldNode) -> El {
    let state = scope.state;
    let theme = &state.theme;
    let key = scope.key(&f.key);
    let press = Press {
        instance: scope.instance.clone(),
        element: f.key.clone(),
        pressed: false,
        opens: false,
        within: scope.within.clone(),
    };
    let edit = state.typed(&key, &f.value, &press);
    let colour = theme.color(f.color.as_deref().unwrap_or("text"));
    let mut e = rune::text_input(&edit, theme.text(f.style.as_deref().unwrap_or("body")))
        .single_line()
        .key(key.clone())
        .label(f.label.clone())
        .color(colour)
        .caret(colour, theme.color("focus/40"));
    if let Some(placeholder) = &f.placeholder {
        e = e.placeholder(placeholder.clone(), theme.color("text-muted"));
    }
    if let Some(w) = f.w {
        e = e.w(w);
    }
    match f.align.as_deref() {
        Some("center") => e = e.align(Align::Center),
        Some("end") => e = e.align(Align::End),
        _ => {}
    }
    e = super::reach::tracked(scope, e, &key);
    let searched = (scope.within.as_ref())
        .filter(|within| within.picks)
        .map(|within| within.opener.clone());
    let (typing, value) = (key.clone(), f.value.clone());
    let (submitted, picked) = (key.clone(), searched.clone());
    e.on_edit(rune::edit_access(move |s: &mut State| {
        s.typing(&typing, &value, &press)
    }))
    .on_submit(move |s: &mut State, tx: &mut Tx| match &picked {
        Some(opener) => s.pick_found(opener, 0),
        None => s.done_typing(&submitted, true, tx),
    })
    .on_input_key(move |s: &mut State, pressed: &KeyEvent, tx: &mut Tx| {
        match (&pressed.key, &searched) {
            (Key::Escape, _) => s.done_typing(&key, false, tx),
            (Key::Down | Key::Up, Some(opener)) => {
                let found = s.fields.get_mut().found_items.get(opener);
                let items = found.map_or(&[][..], Vec::as_slice);
                let to = match pressed.key == Key::Down {
                    true => items.first(),
                    false => items.last(),
                };
                if let Some((to, _)) = to {
                    tx.focus(to, true);
                }
            }
            _ => return false,
        }
        true
    })
}

/// Whether `node`, in the open overlay of `scope`, shows: an item of a menu or list shows only
/// while its label has what the overlay's search field says, and a group of items, such as one
/// under a heading, only while one of them shows.
pub(super) fn kept(scope: &Scope, node: &Node) -> bool {
    let Some(within) = scope.within.as_ref().filter(|within| within.picks) else {
        return true;
    };
    let Some(search) = scope.state.search_of(&within.opener) else {
        return true;
    };
    match (item(node), node) {
        (Some((_, label)), _) => has(label, &search),
        (None, Node::Row(b) | Node::Col(b) | Node::Plain(b)) if b.overlay.is_none() => {
            let mut items = Vec::new();
            items_in(b, &mut items);
            items.is_empty() || items.iter().any(|(_, label)| has(label, &search))
        }
        _ => true,
    }
}

fn has(label: &str, search: &str) -> bool {
    label.to_lowercase().contains(search)
}

/// An item of a menu or list: a box a press picks, with its label.
fn item(node: &Node) -> Option<(&BoxNode, &str)> {
    match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) if b.press && b.overlay.is_none() => {
            Some((b, b.label.as_deref()?))
        }
        _ => None,
    }
}

/// Keeps the items the open `overlay` of `scope` shows as its search filters them, and says so
/// in words when none does.
pub(super) fn searched(scope: &Scope, overlay: &BoxNode) -> Option<El> {
    let within = scope.within.as_ref().filter(|within| within.picks)?;
    let search = scope.state.search_of(&within.opener);
    let mut items = Vec::new();
    items_in(overlay, &mut items);
    let found: Vec<(String, Press)> = (items.into_iter())
        .filter(|(_, label)| search.as_deref().is_none_or(|search| has(label, search)))
        .filter_map(|(b, _)| {
            let element = b.key.clone()?;
            let press = Press {
                instance: scope.instance.clone(),
                element: element.clone(),
                pressed: true,
                opens: b.overlay_of(Opens::Press).is_some(),
                within: Some(within.clone()),
            };
            Some((scope.key(&element), press))
        })
        .collect();
    let none = found.is_empty();
    let mut fields = scope.state.fields.borrow_mut();
    fields.found_items.insert(within.opener.clone(), found);
    let search = search.filter(|_| none)?;
    let theme = &scope.state.theme;
    Some(
        text(format!("Nothing matches “{search}”."))
            .key(format!("{}-nothing", within.opener))
            .font(theme.text("label"))
            .color(theme.color("text-muted"))
            .wrap()
            .padding(theme.space(2)),
    )
}

/// The items inside `b`, not in an overlay of its own.
fn items_in<'a>(b: &'a BoxNode, items: &mut Vec<(&'a BoxNode, &'a str)>) {
    for child in &b.children {
        match (item(child), child) {
            (Some(found), _) => items.push(found),
            (None, Node::Row(inner) | Node::Col(inner) | Node::Plain(inner))
                if inner.overlay.is_none() =>
            {
                items_in(inner, items);
            }
            _ => {}
        }
    }
}

impl State {
    /// What the field `key` says now: `value` while nobody types in it, all of it chosen, so
    /// what is typed takes its place.
    fn typed(&self, key: &str, value: &str, press: &Press) -> TextEdit {
        let mut fields = self.fields.borrow_mut();
        let typed = (fields.by_key.entry(key.to_string()))
            .or_insert_with(|| Typed::new(value, press.clone()));
        typed.press = press.clone();
        // One the keys just left keeps what it says until it is committed.
        let idle = !typed.edit.focused && !typed.had_keys && !typed.searches();
        if idle && (typed.from != value || typed.edit.text != value) {
            *typed = Typed::new(value, press.clone());
        }
        typed.edit.clone()
    }

    /// The field `key`, which Rune edits.
    fn typing(&mut self, key: &str, value: &str, press: &Press) -> &mut TextEdit {
        let fields = self.fields.get_mut();
        let typed = (fields.by_key.entry(key.to_string()))
            .or_insert_with(|| Typed::new(value, press.clone()));
        &mut typed.edit
    }

    /// What the search field of the open menu or list that `opener` opened says, in lower case,
    /// when it says something.
    fn search_of(&self, opener: &str) -> Option<String> {
        let fields = self.fields.borrow();
        let mut searches = fields.by_key.values().filter(|typed| {
            (typed.press.within.as_ref())
                .is_some_and(|within| within.picks && within.opener == opener)
        });
        let search = searches.next()?.edit.text.trim().to_lowercase();
        (!search.is_empty()).then_some(search)
    }

    /// Picks the item at `at` that the open menu or list `opener` opened shows now.
    fn pick_found(&mut self, opener: &str, at: usize) {
        let found = self.fields.get_mut().found_items.get(opener);
        if let Some((_, press)) = found.and_then(|items| items.get(at)).cloned() {
            self.press(&press, "press", None);
        }
    }

    /// Whether a view shows a field now.
    pub(crate) fn shows_a_field(&self) -> bool {
        !self.fields.borrow().found.is_empty()
    }

    /// Whether a field has the keys: none of them is a shortcut then.
    pub(crate) fn typing_now(&self) -> bool {
        let fields = self.fields.borrow();
        fields.by_key.values().any(|typed| typed.edit.focused)
            || self.transport.has_keys()
            || (self.panel.agent.as_ref()).is_some_and(|agent| agent.typing())
            || self.typing_in_settings()
    }

    /// Enter (`keep`) or Escape in the field `key`: its code hears what it says, or `null`, and
    /// the keys go back to where they were.
    fn done_typing(&mut self, key: &str, keep: bool, tx: &mut Tx) {
        let Some(typed) = self.fields.get_mut().by_key.get_mut(key) else {
            return;
        };
        typed.had_keys = false;
        let (press, back) = (typed.press.clone(), typed.back.take());
        let said = keep.then(|| json!(typed.edit.text));
        self.press(&press, "commit", Some(said.unwrap_or(Value::Null)));
        // An overlay it was in gives them back to what opened it as it closes.
        if press.within.is_none() {
            match back {
                Some(back) => tx.focus(&back, false),
                None => tx.blur(),
            }
        }
    }
}

/// What every frame does to the fields: one the keys left, or that went while it had them,
/// commits what it says, and one with `focus` that appears after the composer's press takes
/// the keys, its text chosen. Whether a field took them.
pub(crate) fn follow_fields(state: &mut State) -> bool {
    let left: Vec<(Press, String)> = (state.fields.get_mut().by_key.values_mut())
        .filter_map(|typed| {
            let left = typed.had_keys && !typed.edit.focused && !typed.searches();
            typed.had_keys = typed.edit.focused;
            left.then(|| (typed.press.clone(), typed.edit.text.clone()))
        })
        .collect();
    for (press, text) in left {
        state.press(&press, "commit", Some(json!(text)));
    }
    let tools = state.tools.as_ref();
    let found_in = (
        tools.map_or(0, Tools::view_runs),
        tools.map_or(0, Tools::loads),
        state.open.clone(),
        state.shown.clone(),
    );
    let fields = state.fields.get_mut();
    if fields.found_in.as_ref() == Some(&found_in) {
        return false;
    }
    // What a project shows as it opens or its code loads takes no keys.
    let quiet = (fields.found_in.as_ref()).is_none_or(|(_, loads, ..)| *loads != found_in.1);
    fields.found_in = Some(found_in);
    let shown = shown_fields(state);
    let fields = state.fields.get_mut();
    let gone: Vec<String> = (fields.by_key.keys())
        .filter(|key| !shown.contains_key(*key))
        .cloned()
        .collect();
    let mut commits = Vec::new();
    for key in gone {
        if let Some(typed) = fields.by_key.remove(&key)
            && typed.had_keys
            && !typed.searches()
        {
            commits.push((typed.press, typed.edit.text.clone()));
        }
    }
    let mut took = false;
    let found = shown.keys().cloned().collect();
    for (key, (value, press, focus)) in shown {
        if !focus || quiet || fields.found.contains(&key) {
            continue;
        }
        let typed = (fields.by_key.entry(key)).or_insert_with(|| Typed::new(&value, press));
        if !typed.edit.focused {
            (typed.edit.focused, typed.had_keys) = (true, true);
            typed.back = fields.pressed.clone();
            took = true;
        }
    }
    fields.found = found;
    for (press, text) in commits {
        state.press(&press, "commit", Some(json!(text)));
    }
    took
}

/// The fields the views shown draw, by key in the window: what each says, how a press on it
/// goes, and whether it takes the keys as it appears.
fn shown_fields(state: &State) -> HashMap<String, (String, Press, bool)> {
    let mut found = HashMap::new();
    for id in &state.shown {
        if let Some(node) = state.root(id) {
            let prefix = state.prefix_of(id);
            let mut look = Look {
                state,
                instance: id,
                prefix: &prefix,
                found: &mut found,
            };
            look.node(&node, None);
        }
    }
    found
}

struct Look<'a> {
    state: &'a State,
    instance: &'a InstanceId,
    prefix: &'a str,
    found: &'a mut HashMap<String, (String, Press, bool)>,
}

impl Look<'_> {
    fn node(&mut self, node: &Node, within: Option<&Within>) {
        match node {
            Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
                for child in &b.children {
                    let overlay = match child {
                        Node::Row(o) | Node::Col(o) | Node::Plain(o) => {
                            o.overlay.map(|spec| (o, spec))
                        }
                        _ => None,
                    };
                    match (overlay, &b.key) {
                        (None, _) => self.node(child, within),
                        (Some((_, spec)), Some(_)) if spec.open == Opens::Hover => {
                            self.node(child, within)
                        }
                        (Some((o, _)), Some(opener)) => {
                            let opener = format!("{}{opener}", self.prefix);
                            if self.state.open.contains(&opener) {
                                let picks = o.picks();
                                self.node(child, Some(&Within { opener, picks }));
                            }
                        }
                        (Some(_), None) => {}
                    }
                }
            }
            Node::Field(f) => {
                let press = Press {
                    instance: self.instance.clone(),
                    element: f.key.clone(),
                    pressed: false,
                    opens: false,
                    within: within.cloned(),
                };
                let key = format!("{}{}", self.prefix, f.key);
                self.found.insert(key, (f.value.clone(), press, f.focus));
            }
            Node::Memo { key } => {
                if let Some((_, inner)) = self.state.memo_of(self.instance, key) {
                    self.node(&inner, within);
                }
            }
            Node::Text(_) | Node::Shapes(_) | Node::Icon(_) | Node::Child { .. } => {}
        }
    }
}
