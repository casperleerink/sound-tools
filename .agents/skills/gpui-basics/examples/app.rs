//! Compiles against gpui = "=0.2.2". Shows: App/Window/Entity/Context, notify/observe/subscribe,
//! actions + key bindings + focus/tab order, click/key/hover events, scroll + uniform_list,
//! anchored/deferred popover, svg icons via AssetSource, custom font bytes + font features,
//! and a background timer that updates the view.
use std::{borrow::Cow, sync::Arc, time::Duration};

use gpui::{
    AnyView, App, AppContext, Application, AssetSource, Bounds, ClickEvent, Context, Corner,
    Entity, EventEmitter, FocusHandle, Focusable, Font, FontFeatures, FontStyle, FontWeight,
    KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent, Render, ScrollHandle, SharedString,
    Subscription, Timer, TitlebarOptions, UniformListScrollHandle, Window, WindowBounds,
    WindowOptions, actions, anchored, deferred, div, hsla, prelude::*, px, rems, rgb, size, svg,
    uniform_list,
};

// ---------- actions + key bindings ----------
actions!(demo, [Increment, Decrement, ToggleMenu, Quit, Tab, TabPrev]);

// ---------- assets (svg icons are loaded through this) ----------
struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(match path {
            "icons/play.svg" => Some(Cow::Borrowed(include_bytes!("../assets/play.svg"))),
            _ => None,
        })
    }
    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}

// ---------- a plain data entity (no Render). Observers repaint. ----------
struct Counter {
    count: i32,
}
struct CounterChanged(i32);
impl EventEmitter<CounterChanged> for Counter {}
impl Counter {
    fn add(&mut self, delta: i32, cx: &mut Context<Self>) {
        self.count += delta;
        cx.emit(CounterChanged(delta)); // typed event for `subscribe`
        cx.notify(); // "I changed" for `observe`
    }
}

// ---------- the root view ----------
struct Root {
    focus_handle: FocusHandle,
    counter: Entity<Counter>,
    log: Vec<SharedString>,
    menu_open: bool,
    ticks: u64,
    scroll: ScrollHandle,
    list_scroll: UniformListScrollHandle,
    inputs: Vec<FocusHandle>,
    _subs: Vec<Subscription>,
}

impl Root {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let counter = cx.new(|_| Counter { count: 0 });

        // observe: repaint this view whenever `counter` calls cx.notify()
        let observe = cx.observe(&counter, |_this, _counter, cx| cx.notify());
        // subscribe: react to typed events emitted by `counter`
        let subscribe = cx.subscribe(&counter, |this, _counter, ev: &CounterChanged, cx| {
            this.log.push(format!("delta {}", ev.0).into());
            cx.notify();
        });

        // background timer: only the view's own notify makes it repaint
        cx.spawn(async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(500)).await;
                let alive = this.update(cx, |this, cx| {
                    this.ticks += 1;
                    cx.notify();
                });
                if alive.is_err() {
                    break; // view dropped
                }
            }
        })
        .detach();

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);

        Self {
            focus_handle,
            counter,
            log: vec![],
            menu_open: false,
            ticks: 0,
            scroll: ScrollHandle::new(),
            list_scroll: UniformListScrollHandle::new(),
            // explicit tab order
            inputs: (1..=3)
                .map(|i| cx.focus_handle().tab_index(i).tab_stop(true))
                .collect(),
            _subs: vec![observe, subscribe],
        }
    }

    // action handlers have this exact shape
    fn on_increment(&mut self, _: &Increment, _: &mut Window, cx: &mut Context<Self>) {
        self.counter.update(cx, |c, cx| c.add(1, cx));
    }
    fn on_decrement(&mut self, _: &Decrement, _: &mut Window, cx: &mut Context<Self>) {
        self.counter.update(cx, |c, cx| c.add(-1, cx));
    }
    fn on_toggle_menu(&mut self, _: &ToggleMenu, _: &mut Window, cx: &mut Context<Self>) {
        self.menu_open = !self.menu_open;
        cx.notify();
    }
    fn on_tab(&mut self, _: &Tab, window: &mut Window, _: &mut Context<Self>) {
        window.focus_next();
    }
    fn on_tab_prev(&mut self, _: &TabPrev, window: &mut Window, _: &mut Context<Self>) {
        window.focus_prev();
    }
}

impl Focusable for Root {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.counter.read(cx).count;

        // custom font + OpenType features (ss03, cv01, tnum). Family must be loaded or installed.
        let ui_font = Font {
            family: "Verdana".into(),
            features: FontFeatures(Arc::new(vec![
                ("ss03".to_string(), 1),
                ("cv01".to_string(), 1),
                ("tnum".to_string(), 1),
            ])),
            fallbacks: None,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        };

        div()
            .id("root")
            .key_context("Root") // key bindings can be scoped to this context name
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_increment))
            .on_action(cx.listener(Self::on_decrement))
            .on_action(cx.listener(Self::on_toggle_menu))
            .on_action(cx.listener(Self::on_tab))
            .on_action(cx.listener(Self::on_tab_prev))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _window, cx| {
                this.log.push(format!("key {}", ev.keystroke.key).into());
                cx.notify();
            }))
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .bg(rgb(0x141619))
            .text_color(rgb(0xe6e8eb))
            .font(ui_font)
            .text_size(px(14.))
            // ---- header row: svg icon + text + timer ----
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(svg().path("icons/play.svg").size_4().text_color(rgb(0x7dd3fc)))
                    .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("GPUI 0.2.2 basics"))
                    .child(div().text_color(rgb(0x8b9098)).child(format!("ticks {}", self.ticks))),
            )
            // ---- counter row: on_click needs .id() ----
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(button("dec", "-").on_click(cx.listener(|this, _ev: &ClickEvent, _w, cx| {
                        this.counter.update(cx, |c, cx| c.add(-1, cx));
                    })))
                    .child(div().w(px(48.)).text_center().child(format!("{count}")))
                    .child(button("inc", "+").on_click(cx.listener(|this, _ev, _w, cx| {
                        this.counter.update(cx, |c, cx| c.add(1, cx));
                    })))
                    .child(div().text_color(rgb(0x8b9098)).child("cmd-up / cmd-down also work")),
            )
            // ---- popover: anchored + deferred so it paints above later siblings ----
            .child(
                div()
                    .relative()
                    .child(button("menu", "Menu (cmd-m)").on_click(cx.listener(|this, _, _, cx| {
                        this.menu_open = !this.menu_open;
                        cx.notify();
                    })))
                    .when(self.menu_open, |this| {
                        this.child(
                            deferred(
                                anchored()
                                    .anchor(Corner::TopLeft)
                                    .snap_to_window_with_margin(px(8.))
                                    .child(
                                        div()
                                            .occlude() // swallow mouse events behind the popover
                                            .mt_1()
                                            .w(px(180.))
                                            .p_1()
                                            .rounded_md()
                                            .bg(rgb(0x1f2329))
                                            .border_1()
                                            .border_color(rgb(0x2f343b))
                                            .shadow_lg()
                                            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                                this.menu_open = false;
                                                cx.notify();
                                            }))
                                            .child(menu_item("mi-1", "First item"))
                                            .child(menu_item("mi-2", "Second item")),
                                    ),
                            )
                            .with_priority(1),
                        )
                    }),
            )
            // ---- focus / tab order ----
            .child(
                div().flex().gap_2().children(self.inputs.iter().enumerate().map(|(ix, handle)| {
                    div()
                        .id(("focusable", ix))
                        .track_focus(handle)
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(0x2f343b))
                        .focus(|s| s.border_color(rgb(0x7dd3fc))) // style when focused
                        .on_mouse_down(MouseButton::Left, {
                            let handle = handle.clone();
                            move |_, window, _| window.focus(&handle)
                        })
                        .child(format!("tab {}", handle.tab_index))
                        .when(handle.is_focused(window), |d| d.bg(rgb(0x1f2329)))
                })),
            )
            // ---- hover style + hover callback ----
            .child(
                div()
                    .id("hoverable")
                    .p_2()
                    .rounded_md()
                    .bg(rgb(0x1f2329))
                    .hover(|s| s.bg(rgb(0x2a2f36)))
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.log.push("hover".into());
                            cx.notify();
                        }
                    }))
                    .child("hover me"),
            )
            // ---- uniform_list: virtualized rows of equal height ----
            .child(
                div().h(px(120.)).rounded_md().border_1().border_color(rgb(0x2f343b)).child(
                    uniform_list(
                        "rows",
                        200,
                        cx.processor(|_this, range: std::ops::Range<usize>, _window, _cx| {
                            range
                                .map(|ix| div().id(ix).px_2().h(px(24.)).child(format!("row {ix}")))
                                .collect()
                        }),
                    )
                    .track_scroll(self.list_scroll.clone())
                    .h_full(),
                ),
            )
            // ---- event log (children from an iterator) ----
            .child(div().text_color(rgb(0x8b9098)).child("log:"))
            .children(self.log.iter().rev().take(8).cloned())
    }
}

// Small helpers returning plain elements. `Stateful<Div>` because `.id()` was called.
fn button(id: &'static str, label: impl Into<SharedString>) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .bg(rgb(0x2a2f36))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(0x353b43)))
        .active(|s| s.bg(rgb(0x1f2329)))
        .child(label.into())
}

fn menu_item(id: &'static str, label: &'static str) -> impl IntoElement {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_sm()
        .hover(|s| s.bg(rgb(0x2a2f36)))
        .child(label)
}

// A tooltip must be an `AnyView` (an Entity that implements Render).
#[allow(dead_code)]
struct TextTooltip(SharedString);
impl Render for TextTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().px_2().py_1().rounded_md().bg(rgb(0x1f2329)).text_sm().child(self.0.clone())
    }
}
#[allow(dead_code)]
fn tooltip(text: impl Into<SharedString>, cx: &mut App) -> AnyView {
    cx.new(|_| TextTooltip(text.into())).into()
}

fn main() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        // Load a font from bytes (TTF/OTF). Here from disk; in an app use include_bytes!.
        if let Ok(bytes) = std::fs::read("/System/Library/Fonts/Supplemental/Verdana.ttf") {
            cx.text_system().add_fonts(vec![Cow::Owned(bytes)]).ok();
        }

        cx.bind_keys([
            KeyBinding::new("cmd-up", Increment, Some("Root")),
            KeyBinding::new("cmd-down", Decrement, Some("Root")),
            KeyBinding::new("cmd-m", ToggleMenu, Some("Root")),
            KeyBinding::new("tab", Tab, None),
            KeyBinding::new("shift-tab", TabPrev, None),
            KeyBinding::new("cmd-q", Quit, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(720.), px(560.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("gpui basics".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| Root::new(window, cx)),
        )
        .unwrap();
        cx.activate(true);
    });
}

#[allow(dead_code)]
fn unused_helpers() {
    // 16px rem by default: rems(1.0) == px(16.) unless window.set_rem_size(..) was called.
    let _ = rems(1.0);
    let _ = hsla(0.6, 0.8, 0.6, 1.0); // h, s, l, a all 0.0..=1.0
}
