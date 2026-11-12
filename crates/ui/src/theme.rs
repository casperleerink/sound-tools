use gpui::{App, Global, Hsla, Rgba, rgb};

/// The colours of the app. A view names a role, such as [`Theme::text_muted`] or
/// [`Theme::fill`]: what a colour is for. The palette under the roles, a grey scale from the
/// darkest background to the text and `alpha` to lift a surface, is what another theme changes.
/// The hues are for what DESIGN.md gives each of them, and for marks such as a track's colour.
#[derive(Clone, Debug)]
pub struct Theme {
    gray_50: Hsla,
    gray_100: Hsla,
    gray_200: Hsla,
    gray_300: Hsla,
    gray_400: Hsla,
    gray_500: Hsla,
    gray_600: Hsla,
    gray_700: Hsla,
    gray_800: Hsla,
    gray_900: Hsla,
    gray_950: Hsla,
    alpha: Hsla,
    pub blue: Hsla,
    pub sapphire: Hsla,
    pub sky: Hsla,
    pub teal: Hsla,
    pub green: Hsla,
    pub yellow: Hsla,
    pub peach: Hsla,
    pub red: Hsla,
    pub maroon: Hsla,
    pub mauve: Hsla,
    pub pink: Hsla,
    pub lavender: Hsla,
    pub rosewater: Hsla,
    pub flamingo: Hsla,
}

fn c(hex: u32) -> Hsla {
    let rgba: Rgba = rgb(hex);
    rgba.into()
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            gray_50: c(0x0c0d10),
            gray_100: c(0x121317),
            gray_200: c(0x1b1d23),
            gray_300: c(0x292c34),
            gray_400: c(0x363943),
            gray_500: c(0x474b56),
            gray_600: c(0x60646f),
            gray_700: c(0x7c808c),
            gray_800: c(0x989ca8),
            gray_900: c(0xb5b8c2),
            gray_950: c(0xe9ebef),
            alpha: c(0xffffff),
            blue: c(0x7aa7ff),
            sapphire: c(0x5cc0e8),
            sky: c(0x74d3ea),
            teal: c(0x5fd4c4),
            green: c(0x7ee0a0),
            yellow: c(0xf3d27a),
            peach: c(0xf7a26b),
            red: c(0xf7657a),
            maroon: c(0xf08a96),
            mauve: c(0xb894ff),
            pink: c(0xf28fd0),
            lavender: c(0xa9b1ff),
            rosewater: c(0xf5d9d2),
            flamingo: c(0xf0bcbc),
        }
    }

    /// The window.
    pub fn background(&self) -> Hsla {
        self.gray_100
    }

    /// A card, a panel, a menu: what sits on the window.
    pub fn surface(&self) -> Hsla {
        self.gray_200
    }

    /// The display inside a card: darker than what it sits in.
    pub fn well(&self) -> Hsla {
        self.gray_50
    }

    /// A control at rest, or a quiet box.
    pub fn fill(&self) -> Hsla {
        self.alpha_at(0.05)
    }

    /// A fill under the pointer, and the track of a knob or a slider.
    pub fn fill_strong(&self) -> Hsla {
        self.alpha_at(0.10)
    }

    /// The row or the item that is chosen.
    pub fn selected(&self) -> Hsla {
        self.alpha_at(0.08)
    }

    /// A hairline between rows.
    pub fn border(&self) -> Hsla {
        self.alpha_at(0.05)
    }

    /// The outline of a field or a card.
    pub fn border_strong(&self) -> Hsla {
        self.alpha_at(0.10)
    }

    /// Text, and a value: the one bright line.
    pub fn text(&self) -> Hsla {
        self.gray_950
    }

    /// The label of a control.
    pub fn text_secondary(&self) -> Hsla {
        self.gray_800
    }

    /// A hint, a unit, an icon that is off. Under 4.5 : 1 on a card: not for what must be read there.
    pub fn text_muted(&self) -> Hsla {
        self.gray_700
    }

    /// Text on a bright fill, such as a primary button or a tooltip.
    pub fn text_inverse(&self) -> Hsla {
        self.gray_50
    }

    /// The colour called `name` as tools of a project name them: a role such as `text-muted`,
    /// a hue such as `green`, or a step of the palette such as `gray-700`, which older tools use.
    pub fn named(&self, name: &str) -> Option<Hsla> {
        Some(match name {
            "background" => self.background(),
            "surface" => self.surface(),
            "well" => self.well(),
            "fill" => self.fill(),
            "fill-strong" => self.fill_strong(),
            "selected" => self.selected(),
            "border" => self.border(),
            "border-strong" => self.border_strong(),
            "text" => self.text(),
            "text-secondary" => self.text_secondary(),
            "text-muted" => self.text_muted(),
            "text-inverse" => self.text_inverse(),
            "none" => gpui::transparent_black(),
            "gray-50" => self.gray_50,
            "gray-100" => self.gray_100,
            "gray-200" => self.gray_200,
            "gray-300" => self.gray_300,
            "gray-400" => self.gray_400,
            "gray-500" => self.gray_500,
            "gray-600" => self.gray_600,
            "gray-700" => self.gray_700,
            "gray-800" => self.gray_800,
            "gray-900" => self.gray_900,
            "gray-950" => self.gray_950,
            "alpha" => self.alpha,
            "blue" => self.blue,
            "sapphire" => self.sapphire,
            "sky" => self.sky,
            "teal" => self.teal,
            "green" => self.green,
            "yellow" => self.yellow,
            "peach" => self.peach,
            "red" => self.red,
            "maroon" => self.maroon,
            "mauve" => self.mauve,
            "pink" => self.pink,
            "lavender" => self.lavender,
            "rosewater" => self.rosewater,
            "flamingo" => self.flamingo,
            _ => return None,
        })
    }

    fn alpha_at(&self, opacity: f32) -> Hsla {
        let mut color = self.alpha;
        color.a = opacity;
        color
    }
}

impl Global for Theme {}

pub fn install(cx: &mut App) {
    cx.set_global(Theme::dark());
}

pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}

#[cfg(test)]
mod tests {
    use gpui::Rgba;

    use super::*;

    /// WCAG 2 relative luminance of an opaque colour.
    fn luminance(color: Rgba) -> f32 {
        let channel = |value: f32| match value <= 0.040_45 {
            true => value / 12.92,
            false => ((value + 0.055) / 1.055).powf(2.4),
        };
        0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
    }

    /// `over` painted on the opaque `under`.
    fn blend(under: Hsla, over: Hsla) -> Rgba {
        let (under, over) = (Rgba::from(under), Rgba::from(over));
        let mix = |a: f32, b: f32| a * (1. - over.a) + b * over.a;
        Rgba {
            r: mix(under.r, over.r),
            g: mix(under.g, over.g),
            b: mix(under.b, over.b),
            a: 1.,
        }
    }

    fn contrast(text: Hsla, background: Rgba) -> f32 {
        let (text, background) = (luminance(text.into()), luminance(background));
        (text.max(background) + 0.05) / (text.min(background) + 0.05)
    }

    /// 4.5 : 1 is the WCAG AA minimum for text under 18 pt, and every label here is 12 or 14 pt.
    #[test]
    fn text_roles_have_at_least_the_contrast_of_wcag_aa_on_their_surface() {
        let theme = Theme::dark();
        let opaque = |color: Hsla| blend(color, color);
        let (window, card) = (theme.background(), theme.surface());
        let cases = [
            ("a label on a card", theme.text_secondary(), opaque(card)),
            ("a value on a card", theme.text(), opaque(card)),
            (
                "muted text on the window",
                theme.text_muted(),
                opaque(window),
            ),
            // A toggle that is off, or a segment that is not picked.
            (
                "a label on a fill on a card",
                theme.text_secondary(),
                blend(card, theme.fill()),
            ),
            (
                "a label on a fill on the window",
                theme.text_secondary(),
                blend(window, theme.fill()),
            ),
            (
                "a value on a strong fill on a card",
                theme.text(),
                blend(card, theme.fill_strong()),
            ),
            (
                "a primary button",
                theme.text_inverse(),
                opaque(theme.text()),
            ),
        ];
        for (what, text, background) in cases {
            let ratio = contrast(text, background);
            assert!(ratio >= 4.5, "{what}: {ratio:.2} : 1");
        }
        // An icon needs 3 : 1 (WCAG 1.4.11), and muted text on a card has that and no more:
        // why a label there is `text_secondary`.
        let muted_on_card = contrast(theme.text_muted(), opaque(card));
        assert!(
            (3. ..4.5).contains(&muted_on_card),
            "{muted_on_card:.2} : 1"
        );
    }
}
