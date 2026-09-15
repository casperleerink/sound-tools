use gpui::{App, Hsla, IntoElement, RenderOnce, SharedString, Styled, Svg, Window, px, svg};

/// A lucide icon from `crates/ui/assets/icons`. Path is the file name without `.svg`.
/// Defaults to 16 px and the inherited text colour.
#[derive(IntoElement)]
pub struct Icon {
    base: Svg,
    size: f32,
    color: Option<Hsla>,
}

impl Icon {
    pub fn new(name: impl Into<SharedString>) -> Self {
        let name: SharedString = name.into();
        Self {
            base: svg().path(SharedString::from(format!("icons/{name}.svg"))),
            size: 16.,
            color: None,
        }
    }

    pub fn size(mut self, px_size: f32) -> Self {
        self.size = px_size;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl Styled for Icon {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for Icon {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        // `svg` paints with its own text colour only; it does not inherit from the parent.
        let color = self.color.unwrap_or_else(|| window.text_style().color);
        self.base.size(px(self.size)).flex_none().text_color(color)
    }
}
