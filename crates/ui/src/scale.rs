//! The sizes a view names instead of a number: the corner of a control or a surface, and the
//! height of a control.

use gpui::Pixels;

/// Corner radius. A pill or a circle is gpui's `rounded_full`.
pub mod radius {
    use gpui::{Pixels, px};

    /// A small part inside a control: a segment, a key, a swatch.
    pub const SM: Pixels = px(4.);
    /// A small control, a row of a list.
    pub const MD: Pixels = px(6.);
    /// A card, a menu, a field: a surface and what is as tall as a field.
    pub const LG: Pixels = px(8.);
}

/// The height of a control. Controls of one size line up in a row.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSize {
    /// 24 pt: a control of a device card.
    Sm,
    /// 28 pt.
    #[default]
    Md,
    /// 32 pt: a field or a button of a page.
    Lg,
}

impl ControlSize {
    pub const ALL: [Self; 3] = [Self::Sm, Self::Md, Self::Lg];

    pub const fn height(self) -> f32 {
        match self {
            Self::Sm => 24.,
            Self::Md => 28.,
            Self::Lg => 32.,
        }
    }

    /// The corner of a control this tall.
    pub const fn radius(self) -> Pixels {
        match self {
            Self::Sm => radius::MD,
            Self::Md | Self::Lg => radius::LG,
        }
    }

    /// The corner of a part inside a control this tall, such as a segment: 2 pt in from
    /// [`ControlSize::radius`].
    pub(crate) const fn inner_radius(self) -> Pixels {
        match self {
            Self::Sm => radius::SM,
            Self::Md | Self::Lg => radius::MD,
        }
    }
}
