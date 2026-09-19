//! "Storybook" gallery: one window, one scrolling root view, a section per component with
//! every variant laid out in a row. Run it, screenshot it, compare to the design.
#[path = "components.rs"]
mod components;

use std::borrow::Cow;

use gpui::{
    App, AppContext, AssetSource, Bounds, Context, Entity, FontWeight, Render,
    SharedString, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};

use components::{
    ActiveTheme, Button, ButtonSize, ButtonVariant, LiveBadge, Popover, Switch, Theme,
};

struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(match path {
            "icons/play.svg" => Some(Cow::Borrowed(include_bytes!("../../gpui-basics/assets/play.svg"))),
            _ => None,
        })
    }
    fn list(&self, _: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}

struct Gallery {
    switch_on: bool,
    clicks: usize,
    popover: Entity<Popover>,
}

impl Gallery {
    fn new(cx: &mut Context<Self>) -> Self {
        let popover = cx.new(|cx| Popover::new(vec!["Sine".into(), "Saw".into(), "Square".into()], cx));
        // repaint the gallery when the popover's selection changes
        cx.observe(&popover, |_, _, cx| cx.notify()).detach();
        Self { switch_on: true, clicks: 0, popover }
    }
}

/// A titled section with its variants in a wrapping row.
fn section(title: &'static str, cx: &App, items: impl IntoIterator<Item = impl IntoElement>) -> impl IntoElement {
    let t = cx.theme();
    let (muted, border) = (t.text_muted, t.border);
    div()
        .flex()
        .flex_col()
        .gap_3()
        .pb_6()
        .border_b_1()
        .border_color(border)
        .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(muted).child(title))
        .child(div().flex().flex_wrap().items_center().gap_3().children(items))
}

impl Render for Gallery {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let (bg, text, font) = (t.bg, t.text, t.ui_font.clone());
        let selected = self.popover.read(cx).selected.clone();

        div()
            .id("gallery")
            .overflow_y_scroll()
            .size_full()
            .flex()
            .flex_col()
            .gap_6()
            .p_8()
            .bg(bg)
            .text_color(text)
            .font(font)
            .text_size(px(14.))
            .child(section(
                "Button / variants",
                cx,
                [ButtonVariant::Primary, ButtonVariant::Secondary, ButtonVariant::Ghost, ButtonVariant::Danger]
                    .into_iter()
                    .enumerate()
                    .map(|(ix, v)| {
                        Button::new(("variant", ix), format!("{v:?}"))
                            .variant(v)
                            .tooltip(format!("{v:?} button"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clicks += 1;
                                cx.notify();
                            }))
                    }),
            ))
            .child(section(
                "Button / sizes, icon, disabled",
                cx,
                vec![
                    Button::new("sm", "Small").size(ButtonSize::Sm).variant(ButtonVariant::Secondary),
                    Button::new("md", "Medium").variant(ButtonVariant::Secondary),
                    Button::new("lg", "Large").size(ButtonSize::Lg).variant(ButtonVariant::Secondary),
                    Button::new("icon", "Play").icon("icons/play.svg"),
                    Button::new("disabled", "Disabled").disabled(true),
                    Button::new("wide", "Styled from outside").variant(ButtonVariant::Ghost).w(px(220.)),
                ],
            ))
            .child(section(
                "Switch",
                cx,
                vec![
                    // on_change takes `bool` by value: use cx.processor (Fn(E, ..)), not cx.listener (Fn(&E, ..))
                    Switch::new("switch", self.switch_on).on_change(cx.processor(|this, on: bool, _, cx| {
                        this.switch_on = on;
                        cx.notify();
                    })),
                    Switch::new("switch-disabled", true).disabled(true),
                ],
            ))
            .child(section("Badge / animation", cx, vec![LiveBadge::new("recording")]))
            .child(section("Popover (stateful view)", cx, vec![self.popover.clone()]))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().text_muted)
                    .child(format!("clicks {} · selected {}", self.clicks, selected.unwrap_or_default())),
            )
    }
}

fn main() {
    gpui_platform::application().with_assets(Assets).run(|cx: &mut App| {
        cx.set_global(Theme::dark());
        let bounds = Bounds::centered(None, size(px(760.), px(600.)), cx);
        cx.open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
            |_, cx| cx.new(Gallery::new),
        )
        .unwrap();
        cx.activate(true);
    });
}
