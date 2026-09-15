//! Switch: a pill track with a sliding thumb, sizes 20/24/32 px tall. Controlled — the caller owns
//! `checked` and gets an `on_change(bool)`. The thumb slides over 100 ms; GPUI has no CSS
//! transitions, so the animation is replayed by keying it on the checked state.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, App, ClickEvent, Div, ElementId, FocusHandle, Interactivity,
    KeyDownEvent, StyleRefinement, Window, div, ease_out_quint, prelude::*, px,
};

use crate::theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SwitchSize {
    Sm,
    #[default]
    Md,
    Lg,
}

impl SwitchSize {
    /// Track height, track width, thumb size.
    fn metrics(self) -> (f32, f32, f32) {
        match self {
            Self::Sm => (20., 36., 16.),
            Self::Md => (24., 44., 20.),
            Self::Lg => (32., 56., 28.),
        }
    }
}

type ChangeHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Switch {
    base: Div,
    id: ElementId,
    checked: bool,
    size: SwitchSize,
    disabled: bool,
    focus_handle: Option<FocusHandle>,
    on_change: Option<ChangeHandler>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            base: div(),
            id: id.into(),
            checked,
            size: SwitchSize::default(),
            disabled: false,
            focus_handle: None,
            on_change: None,
        }
    }

    pub fn size(mut self, size: SwitchSize) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn focus_handle(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }

    pub fn on_change(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl Styled for Switch {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Switch {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl RenderOnce for Switch {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (on_track, off_track, thumb_color, ring) = (
            theme.gray_950,
            theme.alpha_at(0.10),
            theme.gray_50,
            theme.blue,
        );
        let (height, width, thumb) = self.size.metrics();
        let pad = (height - thumb) / 2.;
        let travel = width - thumb - pad * 2.;
        let checked = self.checked;
        let disabled = self.disabled;
        let on_change = self.on_change.filter(|_| !disabled);

        let thumb = div()
            .absolute()
            .top(px(pad))
            .size(px(thumb))
            .rounded_full()
            .bg(thumb_color)
            .with_animation(
                ("switch-thumb", checked as usize),
                Animation::new(Duration::from_millis(100)).with_easing(ease_out_quint()),
                move |el, delta| {
                    let progress = if checked { delta } else { 1. - delta };
                    el.left(px(pad + travel * progress))
                },
            );

        self.base
            .id(self.id)
            .relative()
            .flex_none()
            .h(px(height))
            .w(px(width))
            .rounded_full()
            .bg(if checked { on_track } else { off_track })
            .border_1()
            .border_color(gpui::Hsla::transparent_black())
            .when(disabled, |d| d.opacity(0.4).cursor_not_allowed())
            .when(!disabled, |d| d.cursor_pointer())
            .when_some(self.focus_handle.as_ref(), |d, handle| {
                d.track_focus(handle)
                    .focus(|s| s.border_color(ring.opacity(0.7)))
            })
            .child(thumb)
            .when_some(on_change, |d, f| {
                let key_f = f.clone();
                d.on_click(move |_: &ClickEvent, window, cx| f(!checked, window, cx))
                    .on_key_down(move |ev: &KeyDownEvent, window, cx| {
                        if matches!(ev.keystroke.key.as_str(), "space" | "enter") {
                            key_f(!checked, window, cx);
                        }
                    })
            })
    }
}
