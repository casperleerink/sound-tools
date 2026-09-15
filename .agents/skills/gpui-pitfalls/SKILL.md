---
name: gpui-pitfalls
description: Concrete gotchas for gpui 0.2.2 - renamed APIs that make online examples fail to compile, closure/borrow traps with cx.listener and entity.update, missing repaint, element id collisions, deferred/anchored, text truncation and sizing, child vs children, SharedString, px vs rems, plus how to run + screenshot on macOS and write a #[gpui::test]. Use when a build fails, a click does nothing, the screen does not update, or before verifying UI visually.
---

# gpui 0.2.2 pitfalls

Checked against `~/.cargo/registry/src/*/gpui-0.2.2`. Zed's own repo (`~/Desktop/code/zed`) uses
a newer in-tree gpui whose files differ from the registry crate; copy patterns from it, but
verify every name here. Project builds with `cargo build -p gallery` from the repo root.

## 1. Names that changed (online examples vs 0.2.2)

| Old (blog posts, gpui-component, old Zed) | 0.2.2 |
|---|---|
| `WindowContext` | gone; use `(window: &mut Window, cx: &mut App)` |
| `ViewContext<V>` / `ModelContext<T>` | `Context<V>` (one type for both) |
| `View<V>` / `Model<T>` / `WeakView` | `Entity<V>` / `WeakEntity<V>` |
| `cx.build_view(..)`, `cx.new_view(..)`, `cx.new_model(..)` | `cx.new(\|cx\| ..)` |
| `cx.view()` / `cx.model()` / `cx.handle()` | `cx.entity()` / `cx.weak_entity()` |
| `FocusableView` | `Focusable` (`fn focus_handle(&self, cx: &App) -> FocusHandle`) |
| `cx.focus(&handle)` / `cx.focus_next()` | `window.focus(&handle)` / `window.focus_next()` |
| `cx.open_window(opts, \|cx\| ..)` | `cx.open_window(opts, \|window, cx\| ..)` -> `Result<WindowHandle<V>>` |
| `App::new().run(..)` | `Application::new().run(..)` (`App` is now the context type) |
| `WindowOptions { title: .. }` | `titlebar: Some(TitlebarOptions { title: Some(..), ..Default::default() })` |
| `cx.spawn(\|this, mut cx\| async move {..})` | `cx.spawn(async move \|this, cx\| {..})` (async closure, edition 2024) |
| `.focus_visible(..)` | not present; use `.focus(..)` / `.in_focus(..)` |
| `Render::render(&mut self, cx)` | `render(&mut self, window: &mut Window, cx: &mut Context<Self>)` |
| `RenderOnce::render(self, cx)` | `render(self, window: &mut Window, cx: &mut App)` |
| `on_click(\|ev, cx\|)` | `on_click(\|ev: &ClickEvent, window, cx\|)`; `ClickEvent` is an enum, use `ev.position()`/`ev.modifiers()` |
| `cx.add_window` (app) | only exists on `TestAppContext` |
| `#[derive(IntoElement)]` needing `Component` impl | derive only; it generates `IntoElement` for any `RenderOnce` type |
| `impl Element` with `layout/paint` | `request_layout / prepaint / paint` + `id()` + `source_location()` |
| `element.into_any()` | `.into_any_element()` |

Missing entirely in 0.2.2: text input widget, scrollbar element, checkbox/switch, modal, tabs,
select, toast, `Icon` type, `h_flex()/v_flex()`. Those are all `crates/ui` work.

## 2. Closures and borrows

- `cx.listener(|this, ev, window, cx| ..)` produces `Fn(&E, &mut Window, &mut App)`. If the
  component callback takes the event by value (`Fn(bool, ..)`), use `cx.processor(..)`.
- Both capture a handle to the view, so you can call them in `render` freely. Anything else you
  move in must be `'static`: clone `SharedString`/`Entity` before the closure, use `move`.
- `let t = cx.theme();` borrows `cx` immutably; copy the fields you need into locals before calling
  anything that takes `&mut cx` (`cx.listener`, `cx.new`). `Hsla` is `Copy`.
- `entity.update(cx, ..)` on the entity you are already inside panics:
  "cannot update <T> while it is already being updated" (`entity_map.rs`). Mutate `self` instead.
  Same for `entity.read(cx)` on an entity that is mid-update. Reading child entities from a parent
  view is fine.
- Partial moves out of `self` in `RenderOnce::render` are fine (no `Drop` impl); keep fields
  `Option<..>` so `.take()`/`when_some` work.
- `cx.observe(..)` returns a `Subscription`; if you neither store nor `.detach()` it, the observer
  is dropped immediately and nothing updates.
- `WeakEntity::update` returns `Result`; in spawned loops `break` on `Err` (view was dropped).
- Avoid `cx.spawn` loops that never end; store the `Task` or make it exit on `Err`.
- `Rc<dyn Fn>` for stored callbacks; `Box<dyn Fn>` also works but cannot be cloned into `on_mouse_up` + `on_click`.

## 3. Nothing repaints / click does nothing

- Forgot `cx.notify()` after mutating state, or the mutated entity is not a view and nobody observes it.
- `on_click`/`active`/`tooltip`/`on_hover`/`overflow_scroll` silently unavailable? You need `.id(..)` first.
- Two siblings with the same `ElementId` share hover/active/scroll state. In loops use `("name", ix)`.
- Click lands on the wrong element: a popover without `.occlude()` lets clicks through; a full-size
  overlay without `deferred` paints under later siblings. `deferred(..).with_priority(n)` orders overlays.
- `anchored()` without `.position(..)` uses its layout position; give it `.snap_to_window_with_margin(px(8.))`
  so it flips inside the window. `Corner::{TopLeft,TopRight,BottomLeft,BottomRight}`.
- Key bindings do nothing: no element in the focus path has `.track_focus(&handle)`, the window's
  focus is `None` (call `window.focus(&handle)` once at startup), the binding's context name does
  not match a `.key_context("Name")` ancestor, or `.on_action` sits on an element outside the focus path.
- Tab does nothing: gpui does not bind `tab` itself; bind `tab`/`shift-tab` to actions that call
  `window.focus_next()` / `focus_prev()`, and give handles `.tab_index(n).tab_stop(true)`.
- Screen stale but state changed (see gpui-basics section 2): on macOS drawing runs off a
  CVDisplayLink that gpui stops when the window is occluded/hidden. A covered window keeps its
  last frame; `screencapture -l` returns that stale frame. Bring the window to front before
  screenshotting; resize/re-activate to force a frame. Verified with a traced build: renders per
  timer tick while visible, zero renders after the app was hidden and unhidden, until re-activated.
- `window.refresh()` / `cx.refresh_windows()` only mark dirty; they do not bypass the display link.

## 4. Layout and text

- `overflow_hidden()` clips children but does not shrink text. For ellipsis: `.truncate()` on the
  text container plus `.min_w_0()` (and usually `.flex_1()`) on it inside a flex row. `.line_clamp(n)` for multi-line.
- Text has no intrinsic min width in a flex row; unbounded text pushes siblings out. Fix the text
  container width or add `.min_w_0()`.
- `.text_size(px(13.))` sets font size; line height defaults from the font; set `.line_height(px(20.))`
  when matching a design that specifies both. `.text_sm()` etc. are rems (0.875rem = 14px).
- `px` vs `rems`: helpers like `p_4`, `gap_2`, `rounded_md`, `text_sm` are rem-based (16px rem).
  Use `px(..)` for pixel-exact design values (`.h(px(30.))`); use helpers for rhythm.
- `.size_full()` needs the parent to have a definite size; the root window div gets it for free.
  `uniform_list` and scroll containers need a definite height (`.h(px(..))` or `.flex_1().min_h_0()` in a column).
- `.absolute()` positions relative to the nearest `.relative()` ancestor (like CSS).
- `.child(x)` takes one `impl IntoElement`; `.children(iter)` takes an iterator (`Vec`, `Option`,
  `map(..)`). Mixed element types in one `Vec` need `.into_any_element()`.
- `SharedString` is an `Arc<str>`-like handle: cheap to clone, `From<&'static str>`, `From<String>`
  (allocates). Store labels as `SharedString` in views and `.clone()` in render; `format!(..)` as a
  child is fine but allocates every frame.
- `svg()` needs an `AssetSource` registered with `Application::with_assets(..)`; an unknown path
  renders nothing and logs nothing. Color comes from `.text_color(..)`.
- Custom fonts: `add_fonts` accepts TTF/OTF bytes; the family name is the one inside the font
  (check `cx.text_system().all_font_names()`), and it must be loaded before the first frame.
- `runtime_shaders` feature is required on this Mac (no Xcode Metal toolchain); keep it in the workspace dep.

## 5. Run and screenshot on macOS (no headless mode)

```sh
cargo build -p gallery
./.agents/skills/gpui-pitfalls/screenshot.sh gallery /tmp/gallery.png 3   # from repo root, CARGO_TARGET_DIR aware
# manual equivalent:
target/debug/gallery & PID=$!; sleep 3
osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $PID) to true"
WID=$(swift .agents/skills/gpui-pitfalls/winid.swift gallery | head -1 | cut -f1)   # owner name = binary name
screencapture -x -l "$WID" /tmp/gallery.png; kill $PID
```
- `screencapture` needs Screen Recording permission for the process that runs it (System Settings >
  Privacy & Security > Screen Recording). Without it you get "could not create image from window";
  that is the permission, not gpui. Ask the user to grant it to the terminal/agent host once.
- Use a fixed `WindowBounds::Windowed(Bounds::centered(None, size(px(1440.), px(900.)), cx))` so
  screenshots are comparable; `screencapture -R x,y,w,h` is an alternative when the id lookup fails.
- Interaction from scripts: `osascript`/System Events clicks need Accessibility permission; gpui
  windows expose no accessibility tree for custom elements, so click by coordinates.
- Quit cleanly: `cx.on_window_closed(|cx| if cx.windows().is_empty() { cx.quit() }).detach()` and a `cmd-q` action.

## 6. Unit-ish tests with `TestAppContext`

`gpui` has a `test-support` feature (`[dev-dependencies] gpui = { version = "=0.2.2", features = ["test-support"] }`)
and a `#[gpui::test]` macro. The registry crate's own test lives in `gpui-0.2.2/tests/action_macros.rs`.
```rust
#[gpui::test]
fn toggles_on_click(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| Root::new(window, cx));   // (Entity<Root>, &mut VisualTestContext)
    cx.simulate_keystrokes("cmd-up");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |root, _| root.count), 1);
    cx.simulate_click(gpui::point(gpui::px(20.), gpui::px(20.)), gpui::Modifiers::default());
}
```
`VisualTestContext` also has `simulate_input(str)`, `simulate_mouse_move/down/up`,
`simulate_resize(size)`, `dispatch_action(action)`, `update(|window, cx| ..)`, `draw(..)`, `debug_bounds(selector)`;
`TestAppContext` has `update`, `read`, `set_global`, `executor()`, `notifications(&entity)`, `events(&entity)`.
This test snippet is written against the 0.2.2 signatures but was not compiled here: the
`test-support` feature pulls `rand 0.9`, which is not in the offline cargo cache (unverified until
`cargo test -p sound-ui` runs online once). Prefer type-level correctness and the gallery screenshot
loop for UI; use tests for pure state (theme math, model logic) that does not need a window.
