//! Reusable component patterns for gpui (Zed v1.20.2): Theme global, RenderOnce builder components
//! (Button, Switch, Badge), a Tooltip view, a stateful Popover view, and a pulse animation.
use std::{rc::Rc, sync::Arc, time::Duration};

use gpui::{
    Animation, AnimationExt, AnyView, App, AppContext, ClickEvent, Context, Anchor, Div, ElementId,
    FocusHandle, Font, FontFeatures, FontStyle, FontWeight, Global, Hsla, Interactivity,
    MouseDownEvent, Render, SharedString, StyleRefinement, Window, anchored, deferred, div, hsla,
    prelude::*, pulsating_between, px, svg,
};

// ============================================================ theme
#[derive(Clone)]
pub struct Theme {
    pub bg: Hsla,
    pub surface: Hsla,
    pub surface_hover: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub accent: Hsla,
    pub accent_text: Hsla,
    pub danger: Hsla,
    pub ui_font: Font,
}
impl Global for Theme {}

impl Theme {
    pub fn dark() -> Self {
        Self {
            bg: hsla(0.6, 0.08, 0.09, 1.0),
            surface: hsla(0.6, 0.08, 0.14, 1.0),
            surface_hover: hsla(0.6, 0.08, 0.19, 1.0),
            border: hsla(0.6, 0.08, 0.24, 1.0),
            text: hsla(0.6, 0.05, 0.92, 1.0),
            text_muted: hsla(0.6, 0.05, 0.60, 1.0),
            accent: hsla(0.55, 0.85, 0.65, 1.0),
            accent_text: hsla(0.6, 0.08, 0.09, 1.0),
            danger: hsla(0.0, 0.75, 0.60, 1.0),
            ui_font: Font {
                family: ".SystemUIFont".into(), // special name = platform UI font
                features: FontFeatures(Arc::new(vec![("tnum".to_string(), 1)])),
                fallbacks: None,
                weight: FontWeight::NORMAL,
                style: FontStyle::Normal,
            },
        }
    }
}

/// `cx.theme()` on both `App` and `Context<T>` (Context derefs to App).
pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}
impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}

// ============================================================ button
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ButtonVariant {
    #[default]
    Primary,
    Secondary,
    Ghost,
    Danger,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ButtonSize {
    Sm,
    #[default]
    Md,
    Lg,
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Stateless builder component. `#[derive(IntoElement)]` + `impl RenderOnce` is the whole recipe.
#[derive(IntoElement)]
pub struct Button {
    base: Div, // holds user styling from `Styled`/`InteractiveElement` calls
    id: ElementId,
    label: SharedString,
    variant: ButtonVariant,
    size: ButtonSize,
    disabled: bool,
    icon: Option<SharedString>,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            id: id.into(),
            label: label.into(),
            variant: ButtonVariant::default(),
            size: ButtonSize::default(),
            disabled: false,
            icon: None,
            tooltip: None,
            on_click: None,
        }
    }
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }
    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// Asset path resolved through your `AssetSource` (e.g. "icons/play.svg").
    pub fn icon(mut self, path: impl Into<SharedString>) -> Self {
        self.icon = Some(path.into());
        self
    }
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

// Expose the builder traits by delegating to `base`. Users can now write
// `Button::new(..).w_full().mt_2()` and `.on_mouse_down(..)`.
impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}
impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let (bg, fg, hover, border) = match self.variant {
            ButtonVariant::Primary => (t.accent, t.accent_text, t.accent.opacity(0.85), t.accent),
            ButtonVariant::Secondary => (t.surface, t.text, t.surface_hover, t.border),
            ButtonVariant::Ghost => (Hsla::transparent_black(), t.text, t.surface_hover, Hsla::transparent_black()),
            ButtonVariant::Danger => (t.danger, t.accent_text, t.danger.opacity(0.85), t.danger),
        };
        let active = bg.opacity(0.7);
        let (height, pad_x, text_size) = match self.size {
            ButtonSize::Sm => (px(24.), px(8.), px(12.)),
            ButtonSize::Md => (px(30.), px(12.), px(13.)),
            ButtonSize::Lg => (px(36.), px(16.), px(14.)),
        };
        let disabled = self.disabled;
        let on_click = self.on_click.filter(|_| !disabled);
        let tooltip = self.tooltip;

        self.base
            .id(self.id) // Div -> Stateful<Div>: enables on_click/active/tooltip
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap_1p5()
            .h(height)
            .px(pad_x)
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_color(fg)
            .text_size(text_size)
            .font_weight(FontWeight::MEDIUM)
            .when(disabled, |b| b.opacity(0.5).cursor_not_allowed())
            .when(!disabled, |b| {
                b.cursor_pointer()
                    .hover(|s| s.bg(hover))
                    .active(|s| s.bg(active))
                    .focus(|s| s.border_color(hover))
            })
            .when_some(self.icon, |b, path| {
                b.child(svg().path(path).size_4().flex_none().text_color(fg))
            })
            .child(self.label)
            .when_some(tooltip, |b, text| {
                b.tooltip(move |_window, cx| Tooltip::view(text.clone(), cx))
            })
            .when_some(on_click, |b, f| {
                b.on_click(move |ev, window, cx| {
                    cx.stop_propagation();
                    f(ev, window, cx)
                })
            })
    }
}

// ============================================================ switch (controlled)
type ChangeHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    checked: bool,
    disabled: bool,
    on_change: Option<ChangeHandler>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self { id: id.into(), checked, disabled: false, on_change: None }
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn on_change(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let (track, knob) = if self.checked { (t.accent, t.accent_text) } else { (t.border, t.text) };
        let checked = self.checked;
        let group = SharedString::from(format!("switch-{}", self.id));

        div()
            .id(self.id)
            .group(group.clone()) // children can style on `group_hover`
            .flex()
            .items_center()
            .w(px(34.))
            .h(px(20.))
            .p(px(2.))
            .rounded_full()
            .bg(track)
            .when(self.disabled, |d| d.opacity(0.5).cursor_not_allowed())
            .when(!self.disabled, |d| d.cursor_pointer())
            .child(
                div()
                    .size(px(16.))
                    .rounded_full()
                    .bg(knob)
                    .when(checked, |d| d.ml_auto())
                    .group_hover(group, |s| s.opacity(0.9)),
            )
            .when_some(self.on_change.filter(|_| !self.disabled), |d, f| {
                d.on_click(move |_, window, cx| f(!checked, window, cx))
            })
    }
}

// ============================================================ badge with pulse animation
#[derive(IntoElement)]
pub struct LiveBadge {
    label: SharedString,
}
impl LiveBadge {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self { label: label.into() }
    }
}
impl RenderOnce for LiveBadge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let (accent, muted) = (t.accent, t.text_muted);
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .text_xs()
            .text_color(muted)
            .child(
                div().size_2().rounded_full().bg(accent).with_animation(
                    "live-pulse", // ElementId; unique per animated element
                    Animation::new(Duration::from_millis(1400))
                        .repeat()
                        .with_easing(pulsating_between(0.3, 1.0)),
                    |dot, delta| dot.opacity(delta),
                ),
            )
            .child(self.label)
    }
}

// ============================================================ tooltip (must be a view)
pub struct Tooltip {
    text: SharedString,
}
impl Tooltip {
    pub fn view(text: impl Into<SharedString>, cx: &mut App) -> AnyView {
        cx.new(|_| Tooltip { text: text.into() }).into()
    }
}
impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let (bg, border, text) = (t.surface, t.border, t.text);
        // outer padding keeps the tip away from the cursor
        div().pt_2().pl_2().child(
            div()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(bg)
                .border_1()
                .border_color(border)
                .shadow_md()
                .text_sm()
                .text_color(text)
                .child(self.text.clone()),
        )
    }
}

// ============================================================ popover (stateful view)
/// Needs to remember `open`, so it is an Entity with `Render`, not a RenderOnce.
pub struct Popover {
    focus_handle: FocusHandle,
    open: bool,
    items: Vec<SharedString>,
    pub selected: Option<SharedString>,
}

impl Popover {
    pub fn new(items: Vec<SharedString>, cx: &mut Context<Self>) -> Self {
        Self { focus_handle: cx.focus_handle(), open: false, items, selected: None }
    }
}

impl Render for Popover {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let (surface, border, hover) = (t.surface, t.border, t.surface_hover);
        let label = self.selected.clone().unwrap_or_else(|| "Choose...".into());

        div()
            .relative()
            .track_focus(&self.focus_handle)
            .child(
                Button::new("popover-trigger", label)
                    .variant(ButtonVariant::Secondary)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open = !this.open;
                        cx.notify();
                    })),
            )
            .when(self.open, |d| {
                d.child(
                    // deferred: paint after siblings (on top). anchored: keep inside the window.
                    deferred(
                        anchored().anchor(Anchor::TopLeft).snap_to_window_with_margin(px(8.)).child(
                            div()
                                .occlude()
                                .mt_1()
                                .w(px(180.))
                                .p_1()
                                .rounded_md()
                                .bg(surface)
                                .border_1()
                                .border_color(border)
                                .shadow_lg()
                                .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                    this.open = false;
                                    cx.notify();
                                }))
                                .children(self.items.iter().cloned().enumerate().map(|(ix, item)| {
                                    div()
                                        .id(("popover-item", ix))
                                        .px_2()
                                        .py_1()
                                        .rounded_sm()
                                        .cursor_pointer()
                                        .hover(move |s| s.bg(hover))
                                        .child(item.clone())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.selected = Some(item.clone());
                                            this.open = false;
                                            cx.notify();
                                        }))
                                })),
                        ),
                    )
                    .with_priority(1),
                )
            })
    }
}
