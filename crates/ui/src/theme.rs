use gpui::{App, Global, Hsla, Rgba, rgb};

/// The colour tokens: a grey scale from the source design system and the colours that mean
/// something. The names are roles, see DESIGN.md, "Colour".
#[derive(Clone, Debug)]
pub struct Theme {
    pub gray_50: Hsla,
    pub gray_100: Hsla,
    pub gray_200: Hsla,
    pub gray_300: Hsla,
    pub gray_400: Hsla,
    pub gray_500: Hsla,
    pub gray_600: Hsla,
    pub gray_700: Hsla,
    pub gray_800: Hsla,
    pub gray_900: Hsla,
    pub gray_950: Hsla,
    pub alpha: Hsla,
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
    /// Our own palette since September 25, 2026. DESIGN.md, "Colour", gives the role of each.
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

    /// `alpha` at the given opacity, like Tailwind's `alpha/10`.
    pub fn alpha_at(&self, opacity: f32) -> Hsla {
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
    fn text_tokens_have_at_least_the_contrast_of_wcag_aa_on_their_surface() {
        let theme = Theme::dark();
        let opaque = |color: Hsla| blend(color, color);
        let window = opaque(theme.gray_100);
        let card = opaque(theme.gray_200);
        // A toggle that is off, or a segment that is not picked, on a card or on the window.
        let off_on_card = blend(theme.gray_200, theme.alpha_at(0.05));
        let off_on_window = blend(theme.gray_100, theme.alpha_at(0.05));
        let cases = [
            ("a control label on a card", theme.gray_800, card),
            ("a value on a card", theme.gray_950, card),
            ("muted text on the window", theme.gray_700, window),
            ("an off toggle on a card", theme.gray_800, off_on_card),
            ("an off toggle on the window", theme.gray_800, off_on_window),
        ];
        for (what, text, background) in cases {
            let ratio = contrast(text, background);
            assert!(ratio >= 4.5, "{what}: {ratio:.2} : 1");
        }
        // Why labels are not `gray_700`: on a card it is under the line.
        assert!(contrast(theme.gray_700, card) < 4.5);
    }
}
