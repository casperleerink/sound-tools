//! Icons of the project, `extensions/ui/icons/<name>.svg`, tinted with a colour of the theme.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use rune::{El, Icon, el};
use serde::Deserialize;
use sound_core::{InstanceId, Problem, Project};
use sound_typescript::window::Tools;

use crate::app::State;

use super::{Node, Scope};

/// Where the icons of a project are, in its folder.
const FOLDER: &str = "extensions/ui/icons/";

/// `<icon>` of `sdk/view.ts`.
#[derive(Deserialize)]
#[cfg_attr(test, serde(deny_unknown_fields))]
pub(crate) struct IconNode {
    name: String,
    size: Option<f32>,
    color: Option<String>,
}

/// The icons read, by name, with the load of the code they were read in: a file is read again
/// only once the code loads again, so an icon added meanwhile shows then.
#[derive(Default)]
pub(crate) struct Icons(RefCell<HashMap<String, (u64, Option<&'static Icon>)>>);

impl Icons {
    fn get(&self, folder: &Path, loads: u64, name: &str) -> Option<&'static Icon> {
        let mut read = self.0.borrow_mut();
        if let Some((at, icon)) = read.get(name)
            && *at == loads
        {
            return *icon;
        }
        // Rune draws only icons that live as long as the program: one whose file did not change
        // is kept, so reloads leak nothing.
        let kept = read.get(name).and_then(|(_, icon)| *icon);
        let icon = (!name.contains(['/', '\\', '.']))
            .then(|| std::fs::read_to_string(folder.join(FOLDER).join(format!("{name}.svg"))).ok())
            .flatten()
            .map(|text| match kept {
                Some(icon) if icon.body == text => icon,
                _ => leaked(name, text),
            });
        read.insert(name.to_string(), (loads, icon));
        icon
    }
}

fn leaked(name: &str, body: String) -> &'static Icon {
    // A square viewBox's width, which every icon of the kit has.
    let view = (body.split_once("viewBox=\""))
        .and_then(|(_, rest)| rest.split(['"', ' ']).nth(2)?.parse().ok())
        .unwrap_or(24.0);
    Box::leak(Box::new(Icon {
        name: Box::leak(name.to_string().into_boxed_str()),
        view,
        body: Box::leak(body.into_boxed_str()),
    }))
}

fn loads(state: &State) -> u64 {
    state.tools.as_ref().map_or(0, Tools::loads)
}

/// An icon, 16 points square unless sized; one without a file takes its room and draws nothing.
pub(super) fn drawn(scope: &Scope, i: &IconNode) -> El {
    let state = scope.state;
    let size = i.size.unwrap_or(16.0);
    match state
        .icons
        .get(state.project().root(), loads(state), &i.name)
    {
        Some(icon) => rune::icon(icon, size)
            .color(state.theme.color(i.color.as_deref().unwrap_or("text")))
            .inert(),
        None => el().size(size, size),
    }
}

/// Says which icons the views shown draw that have no file, among the problems of the project.
pub(crate) fn report_missing(state: &mut State) {
    let mut names = Vec::new();
    for id in &state.shown {
        if let Some(node) = state.root(id) {
            named(state, id, &node, &mut names);
        }
    }
    let (folder, loads) = (state.project().root().to_path_buf(), loads(state));
    let mut missing: Vec<Problem> = (names.into_iter())
        .filter(|name| state.icons.get(&folder, loads, name).is_none())
        .map(|name| Problem {
            path: format!("{FOLDER}{name}.svg"),
            message: format!("a view draws the icon \"{name}\", which has no file here"),
        })
        .collect();
    missing.sort_by(|a, b| a.path.cmp(&b.path));
    missing.dedup_by(|a, b| a.path == b.path);
    if reported(state.project()) != missing {
        (state.session).background(|project| project.set_problems_in(FOLDER, missing));
    }
}

fn reported(project: &Project) -> Vec<Problem> {
    let mut problems: Vec<Problem> = (project.problems().into_iter())
        .filter(|problem| problem.path.starts_with(FOLDER))
        .collect();
    problems.sort_by(|a, b| a.path.cmp(&b.path));
    problems
}

/// The names of the icons in `node` and its memos.
fn named(state: &State, id: &InstanceId, node: &Node, names: &mut Vec<String>) {
    match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
            (b.children.iter()).for_each(|child| named(state, id, child, names));
        }
        Node::Icon(icon) => names.push(icon.name.clone()),
        Node::Memo { key } => {
            if let Some((_, inner)) = state.memo_of(id, key) {
                named(state, id, &inner, names);
            }
        }
        Node::Text(_) | Node::Shapes(_) | Node::Field(_) | Node::Child { .. } => {}
    }
}
