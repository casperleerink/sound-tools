use gpui::{App, Global, Hsla, Rgba, rgb};

/// Catppuccin Mocha mapped onto the source design system's grey scale. See DESIGN.md.
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
    pub fn mocha() -> Self {
        Self {
            gray_50: c(0x11111b),
            gray_100: c(0x181825),
            gray_200: c(0x1e1e2e),
            gray_300: c(0x313244),
            gray_400: c(0x45475a),
            gray_500: c(0x585b70),
            gray_600: c(0x6c7086),
            gray_700: c(0x7f849c),
            gray_800: c(0x9399b2),
            gray_900: c(0xa6adc8),
            gray_950: c(0xcdd6f4),
            alpha: c(0xffffff),
            blue: c(0x89b4fa),
            sapphire: c(0x74c7ec),
            sky: c(0x89dceb),
            teal: c(0x94e2d5),
            green: c(0xa6e3a1),
            yellow: c(0xf9e2af),
            peach: c(0xfab387),
            red: c(0xf38ba8),
            maroon: c(0xeba0ac),
            mauve: c(0xcba6f7),
            pink: c(0xf5c2e7),
            lavender: c(0xb4befe),
            rosewater: c(0xf5e0dc),
            flamingo: c(0xf2cdcd),
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
    cx.set_global(Theme::mocha());
}

pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}
