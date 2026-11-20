//! The design tokens of a project: its `extensions/theme.json`, else the template's. Views name
//! tokens; this turns a name into what Rune draws.

use std::collections::HashMap;
use std::path::Path;

use rune::{Color, TextStyle};
use rune_kit::{Kit, PaletteFocus, PaletteSeed, Radius, StateColors, Type};
use serde::Deserialize;

/// The template's theme, for a project without one of its own.
const TEMPLATE_THEME: &str = include_str!("../../../templates/daw/extensions/theme.json");

#[derive(Deserialize)]
pub(crate) struct Theme {
    color: HashMap<String, String>,
    text: HashMap<String, TextToken>,
    space: HashMap<String, f32>,
    radius: HashMap<String, f32>,
    shadow: HashMap<String, ShadowToken>,
    /// The tokens as rune-kit reads them, for the app's own chrome: made once they are read.
    #[serde(skip, default = "no_kit")]
    pub(crate) kit: Kit,
}

fn no_kit() -> Kit {
    Kit::neutral(true)
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
        let (mut theme, problem) = match own {
            Some(Ok(theme)) => (theme, None),
            Some(Err(error)) => (
                fallback(),
                Some(format!("its design tokens do not read: {error}")),
            ),
            None => (fallback(), None),
        };
        theme.kit = theme.make_kit();
        (theme, problem)
    }

    /// rune-kit's tokens from these, so the chrome and the project's views look like one app.
    fn make_kit(&self) -> Kit {
        let c = |name| self.color(name);
        let seed = PaletteSeed {
            bg: c("background"),
            fg: c("text"),
            accent: c("focus"),
        };
        let mut kit = Kit::from_palette_focus(seed, PaletteFocus::Custom(c("focus")));
        let palette = &mut kit.c;
        (palette.layer_01, palette.layer_02, palette.bg_deep) =
            (c("surface"), c("raised"), c("well"));
        (palette.bg_button_neutral, palette.surface_raised) = (c("surface"), c("surface"));
        (palette.text_muted, palette.text_faint) = (c("text-secondary"), c("text-muted"));
        (palette.icon_muted, palette.icon_faint) = (c("text-secondary"), c("text-muted"));
        (palette.border_muted, palette.border_base) = (c("border"), c("border-strong"));
        (palette.hover, palette.pressed) = (c("fill"), c("fill-strong"));
        for (role, hue) in [
            (&mut palette.success, "green"),
            (&mut palette.warning, "warning"),
            (&mut palette.danger, "danger"),
            (&mut palette.info, "blue"),
        ] {
            *role = StateColors {
                fg: c(hue),
                icon: c(hue),
                ..*role
            };
        }
        let t = |name| self.text(name);
        let medium = |name| TextStyle {
            weight: 500.0,
            ..t(name)
        };
        kit.ty = Type {
            ui: t("body"),
            ui_medium: medium("body"),
            ui_compact: t("body").lh(16.0),
            small: t("label"),
            small_medium: medium("label"),
            body: t("body"),
            body_medium: t("heading"),
            text_small: t("label"),
            text_small_medium: medium("label"),
            text_xsmall: t("label"),
            text_xsmall_medium: medium("label"),
            mono: t("label").mono(),
        };
        let r = |name, or| self.radius(name).unwrap_or(or);
        kit.radius = Radius {
            xs: r("sm", 4.0) / 2.0,
            sm: r("sm", 4.0),
            md: r("md", 6.0),
            lg: r("lg", 8.0),
            xl: r("lg", 8.0) + 2.0,
        };
        kit
    }

    fn empty() -> Theme {
        Theme {
            color: HashMap::new(),
            text: HashMap::new(),
            space: HashMap::new(),
            radius: HashMap::new(),
            shadow: HashMap::new(),
            kit: no_kit(),
        }
    }

    /// A token, `token/percent`, or `#rrggbb[aa]`. A token may name another, as a role names a hue.
    pub(crate) fn color(&self, name: &str) -> Color {
        let (token, percent) = match name.split_once('/') {
            Some((token, percent)) => (token, percent.parse::<f32>().ok()),
            None => (name, None),
        };
        // A role such as `sound` names a colour of the palette, such as `green`.
        let mut hex = Some(token);
        for _ in 0..4 {
            match hex {
                Some(name) if !name.starts_with('#') => {
                    hex = self.color.get(name).map(String::as_str)
                }
                _ => break,
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2 relative luminance of an opaque colour. Rune's colours are linear already.
    fn luminance(color: Color) -> f32 {
        0.2126 * color.r + 0.7152 * color.g + 0.0722 * color.b
    }

    /// `over` painted on the opaque `under`. The window blends in sRGB, as CSS does.
    fn blend(under: Color, over: Color) -> Color {
        let encode = |v: f32| match v <= 0.003_130_8 {
            true => v * 12.92,
            false => 1.055 * v.powf(1. / 2.4) - 0.055,
        };
        let decode = |v: f32| match v <= 0.040_45 {
            true => v / 12.92,
            false => ((v + 0.055) / 1.055).powf(2.4),
        };
        let mix = |a: f32, b: f32| decode(encode(a) * (1. - over.a) + encode(b) * over.a);
        Color {
            r: mix(under.r, over.r),
            g: mix(under.g, over.g),
            b: mix(under.b, over.b),
            a: 1.,
        }
    }

    fn contrast(text: Color, background: Color) -> f32 {
        let (text, background) = (luminance(text), luminance(background));
        (text.max(background) + 0.05) / (text.min(background) + 0.05)
    }

    /// 4.5 : 1 is the WCAG AA minimum for text under 18 pt, and every label here is 12 or 14 pt.
    #[test]
    fn text_roles_have_at_least_the_contrast_of_wcag_aa_on_their_surface() {
        let theme: Theme = serde_json::from_str(TEMPLATE_THEME).unwrap();
        let c = |name| theme.color(name);
        let (window, card) = (c("background"), c("surface"));
        let cases = [
            ("a label on a card", c("text-secondary"), card),
            ("a value on a card", c("text"), card),
            ("muted text on the window", c("text-muted"), window),
            // A toggle that is off, or a segment that is not picked.
            (
                "a label on a fill on a card",
                c("text-secondary"),
                blend(card, c("fill")),
            ),
            (
                "a label on a fill on the window",
                c("text-secondary"),
                blend(window, c("fill")),
            ),
            (
                "a value on a strong fill on a card",
                c("text"),
                blend(card, c("fill-strong")),
            ),
            ("a primary button", c("text-inverse"), c("text")),
        ];
        for (what, text, background) in cases {
            let ratio = contrast(text, background);
            assert!(ratio >= 4.5, "{what}: {ratio:.2} : 1");
        }
        // An icon needs 3 : 1 (WCAG 1.4.11).
        let muted_on_card = contrast(c("text-muted"), card);
        assert!(muted_on_card >= 3., "{muted_on_card:.2} : 1");
    }
}
