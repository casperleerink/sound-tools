//! The design tokens of a project: its `extensions/theme.json`, else the template's. Views name
//! tokens; this turns a name into what Rune draws.

use std::collections::HashMap;
use std::path::Path;

use rune::{Color, TextStyle};
use serde::Deserialize;

/// The template's theme, for a project without one of its own.
const TEMPLATE_THEME: &str = include_str!("../template/extensions/theme.json");

#[derive(Deserialize)]
pub(crate) struct Theme {
    color: HashMap<String, String>,
    text: HashMap<String, TextToken>,
    space: HashMap<String, f32>,
    radius: HashMap<String, f32>,
    shadow: HashMap<String, ShadowToken>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextToken {
    size: f32,
    line_height: f32,
    tracking: f32,
    weight: f32,
    #[serde(default)]
    tabular: bool,
}

#[derive(Deserialize)]
pub(crate) struct ShadowToken {
    pub(crate) y: f32,
    pub(crate) blur: f32,
    pub(crate) color: String,
}

/// What a name no token has draws as: loud, so a wrong name shows.
const MISSING: Color = Color {
    r: 1.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};

impl Theme {
    /// The theme of the project in `folder`, and what is wrong with its file.
    pub(crate) fn of(folder: &Path) -> (Theme, Option<String>) {
        let path = folder
            .join(sound_typescript::FOLDER)
            .join(sound_typescript::next::THEME);
        let own = std::fs::read_to_string(&path)
            .ok()
            .map(|json| serde_json::from_str(&json));
        let fallback = || serde_json::from_str(TEMPLATE_THEME).unwrap_or_else(|_| Theme::empty());
        match own {
            Some(Ok(theme)) => (theme, None),
            Some(Err(error)) => (
                fallback(),
                Some(format!("its design tokens do not read: {error}")),
            ),
            None => (fallback(), None),
        }
    }

    fn empty() -> Theme {
        Theme {
            color: HashMap::new(),
            text: HashMap::new(),
            space: HashMap::new(),
            radius: HashMap::new(),
            shadow: HashMap::new(),
        }
    }

    /// A token, `token/percent`, or `#rrggbb[aa]`.
    pub(crate) fn color(&self, name: &str) -> Color {
        let (token, percent) = match name.split_once('/') {
            Some((token, percent)) => (token, percent.parse::<f32>().ok()),
            None => (name, None),
        };
        let hex = if token.starts_with('#') {
            Some(token)
        } else {
            self.color.get(token).map(String::as_str)
        };
        let colour = hex.and_then(rune::color::hex).unwrap_or(MISSING);
        percent.map_or(colour, |percent| colour.alpha(colour.a * percent / 100.0))
    }

    pub(crate) fn text(&self, name: &str) -> TextStyle {
        let Some(token) = self.text.get(name) else {
            return TextStyle::new(12.0, 400.0);
        };
        let style = TextStyle::new(token.size, token.weight)
            .lh(token.line_height)
            .tracking(token.tracking);
        if token.tabular {
            style.features("'tnum' 1")
        } else {
            style
        }
    }

    pub(crate) fn space(&self, step: u32) -> f32 {
        self.space.get(&step.to_string()).copied().unwrap_or(0.0)
    }

    pub(crate) fn radius(&self, name: &str) -> Option<f32> {
        self.radius.get(name).copied()
    }

    pub(crate) fn shadow(&self, name: &str) -> Option<&ShadowToken> {
        self.shadow.get(name)
    }
}
