//! The scales a view names in place of raw values: room, corners, the sizes of controls and of
//! text. `sdk/drawing.ts` has the names and this side their values.

use gpui::Hsla;
use serde::Deserialize;
pub(crate) use sound_ui::ControlSize;
use sound_ui::scale::radius;
use sound_ui::theme::Theme;
pub(crate) use sound_ui::typography::TextVariant;

/// A name of a scale, or points.
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum Token {
    Named(String),
    Points(f32),
}

/// Room between things and inside them, in points.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(try_from = "Token")]
pub(crate) struct Spacing(pub(crate) f32);

impl TryFrom<Token> for Spacing {
    type Error = String;

    fn try_from(token: Token) -> Result<Self, String> {
        let points = match token {
            Token::Points(points) => points,
            Token::Named(name) => match name.as_str() {
                "xs" => 4.,
                "sm" => 8.,
                "md" => 12.,
                "lg" => 16.,
                "xl" => 24.,
                "2xl" => 32.,
                _ => {
                    return Err(format!(
                        "{name:?} is no gap or padding: \"xs\", \"sm\", \"md\", \"lg\", \"xl\", \"2xl\" or points"
                    ));
                }
            },
        };
        Ok(Self(points))
    }
}

/// How round a corner is, in points.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(try_from = "Token")]
pub(crate) struct Radius(pub(crate) f32);

impl TryFrom<Token> for Radius {
    type Error = String;

    fn try_from(token: Token) -> Result<Self, String> {
        let points = match token {
            Token::Points(points) => points,
            Token::Named(name) => match name.as_str() {
                "sm" => radius::SM.into(),
                "md" => radius::MD.into(),
                "lg" => radius::LG.into(),
                // As GPUI's `rounded_full`.
                "full" => 9999.,
                _ => {
                    return Err(format!(
                        "{name:?} is no radius: \"sm\", \"md\", \"lg\", \"full\" or points"
                    ));
                }
            },
        };
        Ok(Self(points))
    }
}

/// The tone of a text that names none.
pub(crate) fn tone_of(variant: TextVariant) -> Tone {
    match variant {
        TextVariant::Label => Tone::Secondary,
        _ => Tone::Text,
    }
}

/// How bright a text is.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Tone {
    Text,
    Secondary,
    Muted,
}

impl Tone {
    pub(crate) fn color(self, theme: &Theme) -> Hsla {
        match self {
            Self::Text => theme.text(),
            Self::Secondary => theme.text_secondary(),
            Self::Muted => theme.text_muted(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::de::DeserializeOwned;

    use super::*;
    use crate::tree::Color;

    /// The names of the union `name` in the SDK's `drawing.ts`.
    fn names_of(name: &str) -> Vec<&'static str> {
        let source = include_str!("sdk/drawing.ts");
        let declared = (source.split_once(&format!("export type {name} =")))
            .and_then(|(_, rest)| rest.split_once(';'))
            .map(|(declared, _)| declared);
        let names: Vec<&str> = (declared.into_iter())
            .flat_map(|declared| declared.split('"').skip(1).step_by(2))
            .collect();
        assert!(!names.is_empty(), "no type {name} in drawing.ts");
        names
    }

    fn resolves<T: DeserializeOwned>(union: &str) {
        for name in names_of(union) {
            let read = serde_json::from_value::<T>(serde_json::json!(name));
            assert!(read.is_ok(), "{union} {name:?}: {:?}", read.err());
        }
    }

    #[test]
    fn every_name_of_the_sdks_types_resolves_in_the_window() {
        resolves::<Color>("ThemeColor");
        resolves::<Spacing>("Spacing");
        resolves::<Radius>("Radius");
        resolves::<ControlSize>("ControlSize");
        resolves::<TextVariant>("TextVariant");
        resolves::<Tone>("Tone");
    }

    #[test]
    fn a_name_no_scale_has_is_an_error_that_lists_the_scale() {
        let wrong = serde_json::from_value::<Spacing>(serde_json::json!("huge"));
        let error = wrong.map(|_| ()).unwrap_err().to_string();
        assert!(
            error.contains("\"huge\"") && error.contains("\"2xl\""),
            "{error}"
        );
        let points = serde_json::from_value::<Radius>(serde_json::json!(3));
        assert_eq!(points.map(|radius| radius.0).ok(), Some(3.));
    }

    /// `Select` and `NumberBox` are drawn by the SDK, which keeps the heights for them.
    #[test]
    fn the_sdks_controls_are_as_tall_as_the_windows() {
        let height = ControlSize::height;
        let (sm, md, lg) = (ControlSize::Sm, ControlSize::Md, ControlSize::Lg);
        let kept = format!(
            "const CONTROL_HEIGHT = {{ sm: {}, md: {}, lg: {} }} as const;",
            height(sm),
            height(md),
            height(lg)
        );
        assert!(include_str!("sdk/components.ts").contains(&kept), "{kept}");
    }
}
