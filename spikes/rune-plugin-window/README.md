# Spike: a plugin's editor in a Rune window (macOS)

Workstream F in [PORT-MAP.md](../tool-sdk/PORT-MAP.md). Rune at the rev rune-daw pins.

**Verdict: works with workarounds.** Nothing blocks the port on macOS. Windows is untested.

## What runs

`cargo run` opens a Rune main window with two buttons. Each one opens a second Rune window
at run time (`rune::open_window`) and puts a child view into Rune's `NSView`, taken from
`WindowHandle::with_raw_handle` in `on_created`:

- **CLAP editor**: a real third-party plugin, Six Sines 1.2.1 (a JUCE editor), loaded with
  clack in plugin-host's order: create, get size, set parent, show. `--clap PATH` picks
  another. The repo's test plugins make no real view, so they prove nothing here.
- **Stand-in**: a plain `NSView` that logs every mouse and key event, uses the key `a`
  and passes every other key up the responder chain.

Under the child, Rune draws magenta and cyan, swapping every frame. Any frame where Rune
drew over the editor would show those colours in a capture. Every log line goes to stdout.

## Answers

1. **Drawn above Rune: yes.** Rune's layer has `[RawWindowMetalLayer, child layer]`, back
   to front. No magenta or cyan pixel shows in any capture: not while Rune presents
   57 frames/s, not during a live drag-resize of Six Sines, and not during Rune's own
   animated `resize`.
   ([clap-open](evidence/clap-open.png), [clap-resizing-3](evidence/clap-resizing-3.png),
   [standin-redraw-1](evidence/standin-redraw-1.png), [standin-grown](evidence/standin-grown.png))
2. **Mouse and keys: yes.** Clicks arrive at the right point. Six Sines' knob selects
   ([clap-knob](evidence/clap-knob.png)), and the stand-in gets `(200, 150)` for a click
   200, 150 pt into it. JUCE takes the first responder when it shows. A key the editor
   uses stays there (`a` in the stand-in). Every other key, arrows, Escape, Enter and ⌘K
   included, reaches the window's `on_key` with the right modifiers, and its release
   reaches a root `.on_key_up`. No `SpareKeys` forwarding is needed: AppKit's responder
   chain hands the key to Rune's view.
3. **Placement without a callback: yes.** Reading `placement()` in `poll` every frame sees
   every step of a move and a drag-resize. `on_close` runs while `placement()` still
   reads, so the place is saved there. The plugin also follows a drag-resize from that
   poll (`adjust_size`, then `set_size`).
4. **HiDPI: correct.** A 1048×690 pt editor fills 2096×1380 px. The stand-in reports
   backing scale 2. Its 0.5 pt hairlines are single device pixels (0/224, no grey).
5. **Rune issues**: see below.

## Workarounds the port needs

- `chrome(Chrome { full_size_content: false, .. })` on the plugin window. Rune's default
  puts content under the title bar, where the editor would sit under the window's buttons.
- Rune's view is flipped: y counts down from the top. That matters only for a child the
  host places itself.
- Restoring position: save `content_position` and pass it as `WindowOptions::position`.
  Saving `position`, the frame, brings the window back 32 pt too high (issue 1).
- Float above the main window and hide while the app is not active: `setLevel(3)` and
  `setHidesOnDeactivate(true)` on the `NSWindow` behind the raw handle. GPUI's
  `WindowKind::Floating` gave both. Rune's `MacWindowOptions::level` could replace the first.
- `poll` must ask for a wake (`tx.wake_after`) to keep reading `placement()` while the
  window rests.

## Rune issues to file

1. On macOS, `WindowOptions::position` places the content's top-left in a window with a
   title bar, but the docs and `WindowPlacement::position` mean the frame's. A
   `placement()` round trip moves the window up by the title bar's height.
2. No moved/resized callback. Polling every frame works, but costs a wake per frame.
3. No option for "above this app's windows, hidden while another app is active"
   (`hidesOnDeactivate`). The raw handle covers it.

## Not covered

- Windows: an `HWND` child over Rune's surface (GPUI needed a transparent window there),
  physical-pixel sizes, and owner windows. That needs its own spike before porting F.
- VST 3: same `NSView` parenting as CLAP. The host-side key forwarding that VST 3 needs
  (`IPlugView::onKeyDown`) is a port of `vst3/view.rs` and is not tested here.
