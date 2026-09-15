---
name: gpui-basics
description: Core model of gpui 0.2.2 (App, Window, Entity, Context, Render vs RenderOnce), how state reaches the screen (notify/observe/subscribe, the macOS repaint pitfall), focus/actions/key bindings, events, text input, scrolling, layout, popovers, svg icons and fonts. Use before writing any GPUI view or porting a React component to Rust.
---

# gpui 0.2.2 basics

Every API name here was checked against `~/.cargo/registry/src/*/gpui-0.2.2/src`. Full compiling
examples: `examples/app.rs` (everything below in one app) and `examples/text_input.rs`.
Project: `crates/ui` (theme in `crates/ui/src/theme.rs`, fonts `crates/ui/assets/fonts`, lucide
icons `crates/ui/assets/icons`) and `crates/gallery`. Build: `cargo build -p gallery` from repo root.
Workspace deps: `gpui = { version = "=0.2.2", features = ["runtime_shaders"] }`, edition 2024.

## 1. The object model

| Thing | What it is |
|---|---|
| `Application::new().run(\|cx: &mut App\| ..)` | owns everything; `cx: &mut App` is the root context |
| `Entity<T>` | `Rc`-like handle to state owned by the App. `cx.new(\|cx\| T)`, `e.read(cx)`, `e.update(cx, \|t, cx\| ..)`, `e.downgrade()` -> `WeakEntity<T>` (`weak.update(cx, ..)` returns `Result`) |
| `Context<T>` | `App` plus "which entity am I". Derefs to `App`, so any `App` method works on it |
| view | an `Entity<V>` where `V: Render`. `Entity<V>` and `AnyView` are `IntoElement`, so `.child(self.child_view.clone())` |
| `Window` | passed alongside `cx` everywhere: `(&mut Window, &mut App)` or `(&mut Window, &mut Context<V>)` |
| `Render` | `fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement` (stateful view) |
| `RenderOnce` + `#[derive(IntoElement)]` | `fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement` (stateless component, see gpui-components) |

Imports: `use gpui::{prelude::*, ...}`. The prelude brings the traits
(`Styled, InteractiveElement, StatefulInteractiveElement, ParentElement, IntoElement, Render,
RenderOnce, AppContext, VisualContext, FluentBuilder`). Names like `div, px, rems, rgb, hsla, svg,
Context, Window, App, Entity` are imported explicitly from `gpui`.

```rust
use gpui::{App, AppContext, Application, Bounds, Context, TitlebarOptions, Window, WindowBounds, WindowOptions, div, prelude::*, px, size};

struct Root { count: i32 }
impl Render for Root {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_4().child(format!("{}", self.count))
            .child(div().id("inc").child("+").on_click(cx.listener(|this, _ev, _w, cx| { this.count += 1; cx.notify(); })))
    }
}
fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(800.), px(600.)), cx);
        cx.open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions { title: Some("Sound Tools".into()), ..Default::default() }),
            ..Default::default()
        }, |window, cx| cx.new(|cx| Root { count: 0 })).unwrap();   // closure gets (&mut Window, &mut App)
        cx.on_window_closed(|cx| if cx.windows().is_empty() { cx.quit() }).detach();
        cx.activate(true);
    });
}
```
There is no `WindowOptions.title`; it is `titlebar: Some(TitlebarOptions { title, appears_transparent, traffic_light_position })`.

## 2. How state changes reach the screen

- `cx.notify()` inside a `Context<V>` marks the view dirty in every window that rendered it, and runs
  observers. For a plain data entity (no `Render`) `notify` only runs observers: some view must
  observe it and call its own `cx.notify()`.
- `cx.observe(&entity, |this, entity, cx| cx.notify())` -> `Subscription`. Store it in a field
  (`_sub: Subscription`) or `.detach()`; a dropped subscription stops firing.
- Typed events: `impl EventEmitter<MyEvent> for Model {}`, `cx.emit(MyEvent)`,
  `cx.subscribe(&entity, |this, entity, ev: &MyEvent, cx| ..)`.
- `cx.listener(|this, ev, window, cx| ..)` wraps a closure so an element callback gets `&mut Self`
  and `Context<Self>`. It produces `Fn(&E, &mut Window, &mut App)`; use `cx.processor` when the
  callback takes `E` by value.
- Async: `cx.spawn(async move |weak_this, cx| { Timer::after(Duration::from_millis(16)).await;
  weak_this.update(cx, |this, cx| { ..; cx.notify(); }).ok(); }).detach();` (`gpui::Timer`).
- Continuous animation: `window.request_animation_frame()` (notifies the current view next frame)
  or `.with_animation(..)` (see gpui-components).
- Hammers: `window.refresh()` (whole window) and `cx.refresh_windows()` (all windows).
- Globals: `cx.set_global(Theme)`, `cx.global::<Theme>()`, `cx.update_global::<Theme, _>(|t, cx| ..)`,
  `cx.observe_global::<Theme>(|this, cx| ..)`.

### The repaint pitfall (from experiments/core-lifecycle)
The code pattern above is correct; verified with a traced build of `examples/app.rs`: a 500 ms
`cx.spawn` timer + `cx.notify()` produced one `render` per tick while the window was on screen.
Rendering on macOS only happens from a `CVDisplayLink` tick, and gpui stops that link when the
window is occluded (`windowDidChangeOcclusionState` in `src/platform/mac/window.rs`) and restarts
it when the window becomes visible or becomes key (`windowDidBecomeKey`), changes screen, or the
layer is asked to redraw (resize). In the same trace, hiding the app stopped renders and unhiding
it via System Events did not resume them, while notifications kept queueing: exactly the "stale
until resized" symptom. So:
1. Keep the window unobscured and on screen while testing; activate it (`cx.activate(true)` or
   `osascript ... set frontmost`) before taking screenshots.
2. `screencapture -l <id>` of an occluded window returns the last presented frame, not current state.
3. `window.refresh()` only marks the window dirty; the display link still has to run. Resizing or
   re-activating the window (it becomes key) restarts it. `window.activate_window()` and
   `cx.observe_window_activation(..)` exist if you need to hook that.
4. Do not "fix" this with `cx.refresh_windows()` in a timer loop; it does nothing extra.

## 3. Focus, actions, key bindings, tab order

```rust
use gpui::{FocusHandle, Focusable, KeyBinding, actions};
actions!(demo, [Increment, ToggleMenu, Quit]);       // unit-struct actions, namespace `demo`

struct Root { focus_handle: FocusHandle, inputs: Vec<FocusHandle> }
impl Root {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);
        let inputs = (1..=3).map(|i| cx.focus_handle().tab_index(i).tab_stop(true)).collect();
        Self { focus_handle, inputs }
    }
    fn on_increment(&mut self, _: &Increment, _w: &mut Window, cx: &mut Context<Self>) { cx.notify() }
}
impl Focusable for Root { fn focus_handle(&self, _: &App) -> FocusHandle { self.focus_handle.clone() } }
// in render:
div().id("root").key_context("Root").track_focus(&self.focus_handle)
    .on_action(cx.listener(Self::on_increment))
// in main:
cx.bind_keys([KeyBinding::new("cmd-up", Increment, Some("Root")), KeyBinding::new("cmd-q", Quit, None)]);
cx.on_action(|_: &Quit, cx| cx.quit());              // app-level handler
```
- Actions dispatch up the focus path: the focused element's ancestors with `on_action` get it.
  `KeyBinding::new(keys, action, Some("Ctx"))` only fires when an ancestor has `.key_context("Ctx")`.
- Key syntax: `"cmd-k"`, `"shift-tab"`, `"ctrl-cmd-space"`, `"escape"`, `"enter"`, `"left"`.
- Tab order: `FocusHandle::tab_index(n).tab_stop(true)` or on the element `.tab_index(n)`;
  `window.focus_next()` / `window.focus_prev()` bound to `tab` / `shift-tab` yourself (see
  `gpui-0.2.2/examples/tab_stop.rs`). Style: `.focus(|s| s.border_color(..))` (there is no
  `focus_visible` in 0.2.2), `handle.is_focused(window)`, `handle.contains_focused(window, cx)`.
- Programmatic focus in a callback: `window.focus(&handle)`. Blur: `window.blur()`.
- Data actions: `#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)] #[action(namespace = demo)] struct Jump { to: usize }` (needs `serde`, `schemars`).

## 4. Events

```rust
div().id("btn")                                   // .id() is required for on_click/active/tooltip/scroll
    .on_click(cx.listener(|this, ev: &ClickEvent, window, cx| { ev.position(); ev.modifiers(); }))
    .on_mouse_down(MouseButton::Left, |ev: &MouseDownEvent, window, cx| { ev.position; ev.click_count; })
    .on_mouse_up(MouseButton::Left, ..).on_mouse_move(|ev: &MouseMoveEvent, ..| ..)
    .on_mouse_down_out(..)            // click outside (for closing popovers)
    .on_scroll_wheel(|ev: &ScrollWheelEvent, ..| { ev.delta; })
    .on_hover(|hovered: &bool, window, cx| ..)     // stateful only
    .on_key_down(|ev: &KeyDownEvent, ..| { ev.keystroke.key.as_str(); ev.keystroke.modifiers.shift; })
    .on_drag(MyDragData, |data: &MyDragData, offset, window, cx| cx.new(|_| DragPreview))  // preview is a view
    .on_drop(|data: &MyDragData, window, cx| ..)
```
`ClickEvent` is an enum (`Mouse(MouseClickEvent { down, up })` / `Keyboard(..)`); use its
`position()`/`modifiers()` methods. Stop bubbling: `cx.stop_propagation()`; `window.prevent_default()`.
Hover style without id: `.hover(|s| s.bg(..))`. `.active(|s| ..)` needs `.id()`.
Cursor: `.cursor_pointer()`, `.cursor(CursorStyle::IBeam)`.

## 5. Text input (no built-in widget)

gpui 0.2.2 ships no text field. The pattern (from the crate's `examples/input.rs`, copied to
`examples/text_input.rs` here) is:
1. An `Entity<TextInput>` holding `content: SharedString`, `selected_range`, `marked_range`,
   `focus_handle`, and implementing `EntityInputHandler` (IME + `replace_text_in_range`).
2. A custom `Element` (`TextElement { input: Entity<TextInput> }`) whose `paint` calls
   `window.handle_input(&focus_handle, ElementInputHandler::new(bounds, self.input.clone()), cx)`,
   shapes the line with `window.text_system().shape_line(text, font_size, &runs, None)`, and paints
   selection/cursor with `window.paint_quad(fill(bounds, color))`.
3. Editing keys are actions (`Backspace, Left, SelectAll, Paste, ...`) bound with `cx.bind_keys`
   and handled with `.on_action(cx.listener(Self::backspace))` on the wrapper div that
   `.track_focus(&self.focus_handle)` and `.key_context("TextInput")`.
Recommendation: put one copy of that file in `crates/ui/src/components/text_input.rs`, wrap it in
the design-system styling, and reuse it for every text/numeric field. Add the
`unicode-segmentation` crate for grapheme-correct cursor movement (the example here uses
`char_indices` to stay dependency-free). Multi-line editing is a much bigger job; avoid it.

## 6. Scrolling and lists

```rust
let scroll = ScrollHandle::new();                     // store in the view
div().id("panel").overflow_y_scroll().track_scroll(&scroll)  // id required; also overflow_scroll / overflow_x_scroll
scroll.scroll_to_item(ix); scroll.offset(); scroll.set_offset(point);

let list_scroll = UniformListScrollHandle::new();
uniform_list("rows", item_count, cx.processor(|this, range: Range<usize>, window, cx| {
    range.map(|ix| div().id(ix).h(px(24.)).child(format!("row {ix}"))).collect()
})).track_scroll(list_scroll.clone()).h_full()
list_scroll.scroll_to_item(ix, ScrollStrategy::Top);
```
`uniform_list` needs equal-height rows and a parent with a definite height. `list(ListState::new(n,
ListAlignment::Top, px(200.)), |ix, window, cx| .. .into_any_element())` handles variable heights.
There is no scrollbar element; `.scrollbar_width(px(8.))` only reserves space. Draw your own
(Zed's `crates/ui/src/components/scrollbar.rs` is the reference) or skip it for v0.

## 7. Layout and sizing

- Tailwind-style helpers generated by `gpui_macros::style_helpers!`: `flex() flex_col() flex_row()
  flex_1() flex_none() flex_wrap() items_center() justify_between() gap_2() p_4() px_3() py_1()
  m_2() mt_1() ml_auto() w_full() h_full() size_full() w(px(..)) h(rems(..)) min_w_0() max_w_1_2()
  w_1_3() absolute() relative() top_0() left_0() inset_0() overflow_hidden() hidden() invisible()`.
  Numeric suffixes are Tailwind's (`_1` = 4px, `_2` = 8px, `_4` = 16px, `_px` = 1px, `_0p5` = 2px, `_full` = 100%).
- Units: `px(12.)`, `rems(0.75)` (rem = 16px unless `window.set_rem_size(..)`), `relative(0.5)` = 50%.
- Grid: `.grid().grid_cols(3).grid_rows(2)` and children `.col_span(2)`.
- Text: `&str`, `String`, `SharedString` are elements. `.text_xs() .text_sm() .text_base() .text_lg()
  .text_xl()` (0.75/0.875/1/1.125/1.25 rem), `.text_size(px(13.))`, `.font_weight(FontWeight::SEMIBOLD)`,
  `.line_height(px(20.))`, `.text_color(..)`, `.truncate()` (= overflow_hidden + nowrap + ellipsis),
  `.line_clamp(2)`, `.whitespace_nowrap()`, `.text_center()`, `.italic()`, `.underline()`.
  Text does not shrink in flex rows: give the text container `.min_w_0().flex_1()` or a fixed width.
- Popovers/menus: `deferred(anchored().anchor(Corner::TopLeft).snap_to_window_with_margin(px(8.))
  .child(div().occlude()...)).with_priority(1)`. `anchored` positions at its own layout spot unless
  `.position(point)` is given; `deferred` paints after the rest of the tree so it sits on top;
  `.occlude()` stops clicks going through; `.on_mouse_down_out(..)` closes it.
- Tooltips: `.tooltip(|window, cx| cx.new(|_| TooltipView).into())` returns `AnyView` (needs `.id()`).

## 8. SVG icons and images

`svg().path("icons/play.svg").size_4().text_color(color)` (svg takes the text color). The path is
resolved through the app's `AssetSource`, so register one:
```rust
struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(match path { "icons/play.svg" => Some(Cow::Borrowed(include_bytes!("../assets/icons/play.svg"))), _ => None })
    }
    fn list(&self, _: &str) -> gpui::Result<Vec<SharedString>> { Ok(vec![]) }
}
Application::new().with_assets(Assets).run(..)
```
For `crates/ui/assets/icons/*.svg` generate the match arms with a macro or `rust-embed`. Lucide
icons use `stroke="currentColor"`, which gpui's renderer maps to `text_color`. Raster: `img(path_or_bytes)`.

## 9. Fonts and OpenType features (both supported)

```rust
// load bytes (TTF/OTF; not WOFF2: macOS goes through CGFont/font-kit which read SFNT only)
cx.text_system().add_fonts(vec![
    Cow::Borrowed(include_bytes!("../assets/fonts/InterDisplay-Regular.ttf")),
    Cow::Borrowed(include_bytes!("../assets/fonts/InterDisplay-Medium.ttf")),
    Cow::Borrowed(include_bytes!("../assets/fonts/InterDisplay-SemiBold.ttf")),
]).unwrap();            // do this once in Application::run before opening windows
let ui_font = Font {
    family: "Inter Display".into(),   // family name inside the file, not the file name
    features: FontFeatures(Arc::new(vec![("ss03".into(), 1), ("cv01".into(), 1), ("tnum".into(), 1)])),
    fallbacks: None, weight: FontWeight::NORMAL, style: FontStyle::Normal,
};
div().font(ui_font)     // inherited by all children; `.font_family("..")` / `.font_weight(..)` also exist
```
`FontFeatures` is a public tuple struct of `(tag, value)` pairs; macOS applies them via
`apply_features_and_fallbacks` in `platform/mac/open_type.rs` (verified). Check the real family
name with `cx.text_system().all_font_names()`. Add it to the theme (`Theme.ui_font`) and set it once
on the root element of each window so every text run inherits it.
