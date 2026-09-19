---
name: gpui-components
description: How to build a reusable component library in gpui (Zed v1.20.2) (RenderOnce builder components, when to use stateful Entity views, exposing Styled/InteractiveElement on custom types, ElementId rules, theme Global, variants/sizes/disabled, hover/active/focus styling, tooltips, group hover, animation, colors, borders/shadows, and a storybook gallery). Use when porting a React/Tailwind design-system component into crates/ui.
---

# gpui (Zed v1.20.2) components

Compiling reference: `examples/components.rs` (Theme, Button, Switch, LiveBadge, Tooltip, Popover)
and `examples/gallery.rs` (storybook). Project: components live in `crates/ui/src/components/*.rs`,
theme in `crates/ui/src/theme.rs` (`Theme: Global`, `cx.theme()` via `ActiveTheme`), showcase in
`crates/gallery/src/sections/*.rs`. Build: `cargo build -p gallery`.

## 1. Stateless component = `RenderOnce` + `#[derive(IntoElement)]`

```rust
use std::rc::Rc;
use gpui::{App, ClickEvent, Div, ElementId, Interactivity, SharedString, StyleRefinement, Window, div, prelude::*, px, svg};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ButtonVariant { #[default] Primary, Secondary, Ghost, Danger }
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ButtonSize { Sm, #[default] Md, Lg }
type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Button {
    base: Div,                 // absorbs .w_full() / .mt_2() / .on_mouse_down() from callers
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
        Self { base: div(), id: id.into(), label: label.into(), variant: Default::default(),
               size: Default::default(), disabled: false, icon: None, tooltip: None, on_click: None }
    }
    pub fn variant(mut self, v: ButtonVariant) -> Self { self.variant = v; self }
    pub fn size(mut self, s: ButtonSize) -> Self { self.size = s; self }
    pub fn disabled(mut self, d: bool) -> Self { self.disabled = d; self }
    pub fn icon(mut self, path: impl Into<SharedString>) -> Self { self.icon = Some(path.into()); self }
    pub fn tooltip(mut self, t: impl Into<SharedString>) -> Self { self.tooltip = Some(t.into()); self }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f)); self
    }
}
// Expose the builder traits by delegating to `base`.
impl Styled for Button { fn style(&mut self) -> &mut StyleRefinement { self.base.style() } }
impl InteractiveElement for Button { fn interactivity(&mut self) -> &mut Interactivity { self.base.interactivity() } }

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();                                   // read colors first, then drop the borrow
        let (bg, fg, hover, border) = match self.variant { /* .. */ };
        let (height, pad_x, text) = match self.size {
            ButtonSize::Sm => (px(24.), px(8.), px(12.)),
            ButtonSize::Md => (px(30.), px(12.), px(13.)),
            ButtonSize::Lg => (px(36.), px(16.), px(14.)),
        };
        let disabled = self.disabled;
        self.base
            .id(self.id)                                      // Div -> Stateful<Div>
            .flex().flex_none().items_center().justify_center().gap_1p5()
            .h(height).px(pad_x).rounded_md().border_1().border_color(border)
            .bg(bg).text_color(fg).text_size(text).font_weight(FontWeight::MEDIUM)
            .when(disabled, |b| b.opacity(0.5).cursor_not_allowed())
            .when(!disabled, |b| b.cursor_pointer()
                .hover(|s| s.bg(hover))
                .active(|s| s.bg(bg.opacity(0.7)))
                .focus(|s| s.border_color(hover)))
            .when_some(self.icon, |b, path| b.child(svg().path(path).size_4().flex_none().text_color(fg)))
            .child(self.label)
            .when_some(self.tooltip, |b, text| b.tooltip(move |_w, cx| Tooltip::view(text.clone(), cx)))
            .when_some(self.on_click.filter(|_| !disabled), |b, f| {
                b.on_click(move |ev, window, cx| { cx.stop_propagation(); f(ev, window, cx) })
            })
    }
}
```
Rules:
- Callbacks are stored as `Rc<dyn Fn(..)>` (or `Box`) so the struct stays movable; `RenderOnce` consumes `self`.
- Order matters: `.id()` turns `Div` into `Stateful<Div>`; `on_click`, `active`, `tooltip`,
  `on_hover`, `overflow_*_scroll` are only available after it. Styles set on `base` before `.id()` survive.
- `hover`/`active`/`focus` closures receive a `StyleRefinement`, which implements `Styled`, so the same
  `.bg() .border_color() .opacity() .text_color()` helpers work inside them.
- Conditionals: `.when(cond, |el| ..)`, `.when_some(opt, |el, v| ..)`, `.when_else`, `.map(|el| ..)`
  (all from `FluentBuilder` in the prelude).
- Controlled inputs: take `checked: bool` + `on_change(impl Fn(bool, &mut Window, &mut App))`. In a view,
  wire it with `cx.processor(|this, on: bool, _w, cx| { this.on = on; cx.notify(); })` (by-value arg),
  not `cx.listener` (by-reference arg).
- Slots: `children: Vec<AnyElement>` + `impl ParentElement for X { fn extend(&mut self, els: impl IntoIterator<Item = AnyElement>) { self.children.extend(els) } }`
  gives callers `.child(..)`/`.children(..)`. Convert with `.into_any_element()`.

## 2. When to use a stateful `Entity` view instead

Use `struct X { .. } impl Render for X` + `cx.new(|cx| X::new(cx))` when the component owns state
the parent should not manage: open/closed popover or select, text input (needs `FocusHandle` +
`EntityInputHandler`), tabs with an internal active index, drag state, animations driven by timers.
- Parent stores `Entity<X>` and renders `.child(self.x.clone())`.
- Parent reacts by `cx.observe(&self.x, |this, x, cx| cx.notify())` or typed events
  (`impl EventEmitter<Changed> for X {}`, `cx.emit(Changed(..))`, parent `cx.subscribe`).
- Read state with `self.x.read(cx).selected.clone()`; write with `self.x.update(cx, |x, cx| ..)`.
- Inside the view use `cx.listener(|this, ev, window, cx| ..)` for element callbacks.
- Lightweight alternative for tiny per-element state inside a `RenderOnce`:
  `window.use_keyed_state(id, cx, |_, _| S::default())` returns an `Entity<S>` tied to that element id.

Popover pattern (stateful; full code in `examples/components.rs`):
```rust
div().relative()
    .child(Button::new("trigger", label).on_click(cx.listener(|this, _, _, cx| { this.open = !this.open; cx.notify(); })))
    .when(self.open, |d| d.child(
        deferred(anchored().anchor(Anchor::TopLeft).snap_to_window_with_margin(px(8.)).child(
            div().occlude().mt_1().w(px(180.)).p_1().rounded_md().bg(surface).border_1().border_color(border).shadow_lg()
                .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| { this.open = false; cx.notify(); }))
                .children(items),
        )).with_priority(1),
    ))
```

## 3. ElementId rules

- `ElementId` from `&'static str`, `usize`, `(&'static str, usize)`, `(&'static str, u64)`,
  `SharedString`, `Uuid`, `&FocusHandle`. In loops use `("row", ix)`; never `format!` into a
  `&'static str`; if you need a runtime name use `SharedString::from(format!(..))`.
- Ids are scoped by their parent element path and by the rendering view, so the same `"ok"` in two
  different views (or under two different `.id()` parents) does not collide. Duplicate ids among
  siblings do collide: hover/active/scroll state gets mixed up.
- `id()` in a component signature: `id: impl Into<ElementId>`; callers pass `"save"` or `("track", ix)`.
- Animations need their own id: `.with_animation("pulse", ..)`; `group("name")` takes a `SharedString`, not an id.

## 4. Theme

`crates/ui/src/theme.rs` already defines `Theme: Global`, `theme::install(cx)` and
`trait ActiveTheme { fn theme(&self) -> &Theme }` for `App`. Because `Context<T>` derefs to `App`,
`cx.theme()` works in `Render`, `RenderOnce`, listeners and `main`. Read colors into locals before
building the element tree so the immutable borrow of `cx` ends. Add non-color tokens there too
(`ui_font: Font`, radius, spacing) instead of scattering `px(6.)` literals.

## 5. Colors, borders, shadows, states

- `rgb(0x1e1e2e)` -> `Rgba`; `hsla(h, s, l, a)` all `0.0..=1.0`; `Rgba`/`Hsla` both convert into
  `Fill`/`Background` for `.bg()` and into `Hsla` for `.text_color()/.border_color()`.
- `Hsla::opacity(f)` multiplies alpha, `.alpha(a)` sets it, `a.blend(b)` composites `b` over `a`
  (use for "alpha/10 on gray_900" so the result is opaque and cheap), `.grayscale()`, `Hsla::transparent_black()`.
- Gradient: `.bg(linear_gradient(angle_deg, linear_color_stop(c1, 0.), linear_color_stop(c2, 1.)))`.
- Radius: `.rounded_xs/sm/md/lg/xl/2xl/3xl/full()` (2/4/6/8/12/16/24/9999 px), `.rounded(px(5.))`,
  per-corner `.rounded_tl_md()`, per-side `.rounded_t_md()`.
- Borders: `.border_1() .border_2() .border_t_1() .border_x_1() .border(px(1.5))`, `.border_color(c)`, `.border_dashed()`.
- Shadows: `.shadow_2xs/xs/sm/md/lg/xl/2xl()` or `.shadow(vec![BoxShadow { color, offset: point(px(0.), px(2.)), blur_radius: px(8.), spread_radius: px(0.), inset: false }])`.
- Opacity: `.opacity(0.5)` on any element (children included).
- States: `.hover(|s| ..)` (any element), `.active(|s| ..)` (stateful), `.focus(|s| ..)` and
  `.in_focus(|s| ..)` (element or descendant focused; both need `track_focus`), `.group("g")` on
  a parent then `.group_hover("g", |s| ..)` / `.group_active("g", ..)` on children.
  There is no `focus_visible`, `disabled(..)` or `data-*` helper: model disabled yourself
  (`.when(disabled, |d| d.opacity(0.5).cursor_not_allowed())` and drop the click handler).
- Cursor: `.cursor_pointer() .cursor_not_allowed() .cursor_grab() .cursor(CursorStyle::IBeam)`.

## 6. Tooltips and group hover

```rust
pub struct Tooltip { text: SharedString }
impl Tooltip { pub fn view(text: impl Into<SharedString>, cx: &mut App) -> AnyView { cx.new(|_| Tooltip { text: text.into() }).into() } }
impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().pt_2().pl_2().child(div().px_2().py_1().rounded_md().bg(..).border_1().border_color(..).shadow_md().text_sm().child(self.text.clone()))
    }
}
// on any Stateful element:
.tooltip(move |_window, cx| Tooltip::view(text.clone(), cx))   // or .hoverable_tooltip(..)
```
Tooltips are views; gpui positions them near the mouse and handles the delay. Only one
`.tooltip()` per element (debug assert).

## 7. Animation

```rust
use std::time::Duration;
use gpui::{Animation, AnimationExt, pulsating_between, ease_in_out, bounce, linear};
div().size_2().rounded_full().bg(accent)
    .with_animation("live-pulse", Animation::new(Duration::from_millis(1400)).repeat().with_easing(pulsating_between(0.3, 1.0)),
                    |dot, delta| dot.opacity(delta))
```
`with_animation(id, Animation, |element, delta: f32| element)`; `delta` goes 0..1 (eased). Easing
fns: `linear, quadratic, ease_in_out, ease_out_quint(), bounce(f), pulsating_between(min, max)`.
Rotating/scaling an svg: `svg().with_transformation(Transformation::rotate(percentage(delta)))`.
For state-driven transitions (open/close) keep it simple: swap styles; there is no CSS-transition equivalent.

## 8. Storybook gallery

One window, one root view that scrolls, one `section(title, cx, items)` per component with every
variant in a wrapping row (`examples/gallery.rs`; project version in `crates/gallery/src/sections`).
```rust
fn section(title: &'static str, cx: &App, items: impl IntoIterator<Item = impl IntoElement>) -> impl IntoElement {
    let t = cx.theme(); let (muted, border) = (t.text_muted, t.border);
    div().flex().flex_col().gap_3().pb_6().border_b_1().border_color(border)
        .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(muted).child(title))
        .child(div().flex().flex_wrap().items_center().gap_3().children(items))
}
// root: div().id("gallery").overflow_y_scroll().size_full().flex().flex_col().gap_6().p_8().bg(bg).text_color(text).font(ui_font)
//   .child(section("Button / variants", cx, variants.into_iter().enumerate().map(|(ix, v)| Button::new(("variant", ix), format!("{v:?}")).variant(v))))
```
Interactive samples (switch, select) keep their state on the gallery view (`self.switch_on`) or in
child entities (`Entity<Popover>`). Mixed item types in one section: map each to `.into_any_element()`.
Verify visually: `cargo build -p gallery`, run it, screenshot (see gpui-pitfalls), compare with the
design.
