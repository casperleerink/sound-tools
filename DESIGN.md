# Design

Design decisions for the Sound Tools UI. Settled from the three web prototypes reviewed on September 10, 2026 (removed September 14; two screenshots remain in `docs/reference/`). The real UI SDK is built in GPUI in `crates/ui`; the gallery in `crates/gallery` shows every component and variant.

## Source design system

The UI takes its language from an existing web design system (React and Tailwind). The values below are the parts that carry over.

- Font: InterDisplay with `ss03` and `cv01`. Tabular numbers (`tnum`, `lnum`) for every number, meter and time. Monospace stack for code, paths and build output.
- Sizes: controls 24, 28, 32, 40 px tall. Radii 6, 8, 10 px. Body 14 px, labels medium weight, meta 12 px. Agent conversation text 15 or 16 px with 1.5 line height.
- Fills use the alpha scale: `alpha/5` subtle surface, `alpha/10` hover and borders. Disabled is 40% opacity. Inactive tabs 40% opacity, 100% on hover or active.
- Variants: primary (solid text colour on background), subtle, outline, ghost, plus colour variants each with solid, subtle, outline and ghost.
- Motion: 100 ms colour transitions, ease `cubic-bezier(0.08, 0.82, 0.17, 1)`, a slow 3.5 s pulse to 70% opacity for "agent is working".
- Shadows: dropdown `0 4px 24px -8px rgba(0,0,0,0.2)`, card `0 8px 16px -8px rgba(0,0,0,0.1)`.
- Spacing: 8 px grid for layout, 4 px inside controls. Panel and card margins at least 24 px. Card padding at least 16 px.

## Colour: Catppuccin Mocha over the source grey scale

Dark only for now. The source dark theme uses `gray-50` as the darkest background and `gray-950` as text; keep that meaning.

| Token | Mocha | Hex |
| --- | --- | --- |
| gray-50 | crust | `#11111b` |
| gray-100 | mantle | `#181825` |
| gray-200 | base | `#1e1e2e` |
| gray-300 | surface0 | `#313244` |
| gray-400 | surface1 | `#45475a` |
| gray-500 | surface2 | `#585b70` |
| gray-600 | overlay0 | `#6c7086` |
| gray-700 | overlay1 | `#7f849c` |
| gray-800 | overlay2 | `#9399b2` |
| gray-900 | subtext0 | `#a6adc8` |
| gray-950 | text | `#cdd6f4` |
| alpha | white | `#ffffff` |
| blue | blue | `#89b4fa` |
| sapphire | sapphire | `#74c7ec` |
| sky | sky | `#89dceb` |
| teal | teal | `#94e2d5` |
| green | green | `#a6e3a1` |
| yellow | yellow | `#f9e2af` |
| peach | peach | `#fab387` |
| red | red | `#f38ba8` |
| maroon | maroon | `#eba0ac` |
| mauve | mauve | `#cba6f7` |
| pink | pink | `#f5c2e7` |
| lavender | lavender | `#b4befe` |
| rosewater | rosewater | `#f5e0dc` |
| flamingo | flamingo | `#f2cdcd` |

Solid colour buttons need dark text (`#1e1e2e`). Accents are sparse: lavender for the agent, green for play state, peach for warnings, red for errors. Each track or tool instance gets one accent, used on dots, ports and wires only, never on fills.

## Quiet rule

Every element must earn its keep. Reference feel: the source design system and Hive. Lots of air, one accent, calm type. Audio tools are usually crowded; we are not copying that.

- Agent sidebar: a turn is the composer's message and the agent's result text. While working, one line such as `Building Polyrhythm` with a slow pulse. After, a muted `Worked for 12 s` that expands on click to the history. A failed build is `Build failed` in red plus one short sentence. No tool-call rows, progress bars, timestamps per message, or explanatory prose about builds and playback.
- Composer: input, model name, send. Placeholder inside the input is the only hint.
- Transport: floating pill, bottom centre. Play/pause, stop, position, duration if the project has one, a hairline seek strip, the tempo, and the click. Build status and device selection are not in it; at most a small dot when a reload is pending.
- Chrome: project name top-left as a quiet menu holding add, undo/redo, output device and the project folder in the Finder or in a terminal. No legends, no zoom controls, no grid.
- Cards and panels: 16 px padding, no meta chips in headers, port labels on hover only, secondary parameters behind a disclosure or a second view.
- Accessible: visible focus rings, labelled controls, full keyboard reach. This is a product requirement.

## The window, September 19, 2026

What is built, in `crates/runtime/src/window.rs`, `extensions/arrangement/src/view.rs` with `view/editor.rs` and `view/track_panel.rs`, and `extensions/instrument/src/view.rs`. The track panel was added on September 20, 2026. `cargo test -p runtime --test snapshots` renders it to PNGs.

- One background, `gray-100`, for the whole window. No panels and no top bar: the title bar is transparent, the project name sits right of the traffic lights and the row around it drags the window.
- Transport pill: play or pause in green, stop, the position as `bar.beat`, the time as `m:ss` muted, then the hairline seek strip and the duration when the project has an end, then the tempo and the click. Numbers are tabular and the pill sizes from its content, so it stays still while playing and grows by a digit at bar 100 or at ten minutes. Space toggles playback. Tab reaches the buttons, the strip, the tempo and the click, and left and right seek by a bar on the strip.
- Tempo, September 20, 2026: the tempo in effect at the playhead as a plain number in `gray_950` with a muted 12 px `bpm` after it, no box and no fill. Up to three decimals with no zeros at the end, so `120`, `93.5`, `120.125`. Dragging up on the number makes it faster, half a bpm per pixel, landing on whole numbers; with shift a tenth of that. The arrows step by 1 bpm and with shift by 0.1. It shows a 1 px lavender border when the focus came from the keyboard, like the seek strip. There is no tempo lane and no way to add or remove a tempo change in the window: an edit changes the tempo change in effect at the playhead, and the rest is a file edit.
- Click, September 20, 2026: a 28 px icon button at the right end of the pill, the metronome icon. Ghost when it is off, subtle when it sounds, like the mute button of a track: no accent, because the click is a reference and not part of the piece. No volume, no count-in and no sounds to choose from.
- Project menu: add track, undo and redo with the name of the step and their shortcuts, the output device by name with a check, reveal project folder, open terminal in project folder. An item that cannot run is at 40% opacity.
- Arrangement: 176 px track headers with the accent dot and the name at 14 px medium in `gray-900`, 64 px rows, a 32 px ruler with one short mark and one 12 px number in `gray-700` per bar. Bar numbers thin out to every 2nd, 4th, 8th bar when bars get narrow. No grid lines, no row lines, no zoom or scroll controls. Two hairlines at `alpha/5`: under the ruler and right of the headers. Tick 0 sits 8 px into the timeline.
- Clips: `alpha/5` fill with an `alpha/10` hairline border and 6 px corners, 4 px inside the row. The notes are small bars in the accent of the track. That is the one place where a track accent is more than a dot: notes are marks, not fills, and they tie a clip to its track without a label. The selected clip has a `gray-950` border. Clips have no name label.
- Playhead: a 1 px `gray-950` line with a 7 px round head in the ruler.
- The view follows the playhead, September 20, 2026: while the project plays, the arrangement pages forward once the playhead passes the right edge, and the playhead lands back at the left edge. A stop or a seek that leaves the playhead off screen brings it back the same way. While the composer has scrolled the playhead off screen nothing pulls the view back, until the next stop or seek. There is no follow switch, because there is nothing to switch off. The note editor does not follow: it shows one clip.
- Notices: quiet lines bottom-left, 400 px wide at most. A red dot for the last error with a dismiss button that Tab reaches, a peach dot for files that are not live. A long message wraps to at most three lines. Nothing blocks.
- Note editor: a panel of 384 px below the arrangement, on the same background, with one hairline above it. No toolbar and no tools: what the pointer is on decides what a drag does. The header column lines up with the track headers: the accent dot and the name of the track in the ruler row, then a quiet close icon at 60% opacity, and below them a slim key strip of 32 px at the right edge, white keys at `alpha/10` and black keys at `alpha/3`, with only the Cs named in 12 px `gray-700` left of it. Rows are 12 px per semitone. The rows of black keys are tinted `alpha/2`, so a pitch can be read without lines between rows. Bar lines are hairlines at `alpha/5`, beat lines at half of that and only from 24 px per beat. This is the one place with a grid, because notes are placed by it. The arrangement keeps none. Outside the clip the area is a shade darker (`gray-50` at 50%). Notes are rounded bars of 11 px in the track accent with 3 px corners, the selected one filled with `gray-950`, the lightest colour there is, inside its accent outline. An outline alone on a pastel fill was hard to see. The ruler and the playhead are those of the arrangement. The editor opens zoomed to fit its clip, with the middle of its notes in the middle of what the transport leaves free.
- Track panel: the other thing the panel below the arrangement can show, in the same 384 px, so a swap between it and the note editor moves nothing. One at a time, like the clip view and the device view of Ableton. The header is that of the note editor: the accent dot, the name of the track and the quiet close icon, in the same places. Right of the header column is the rack: device cards from left to right, 24 px from the edges, at the top of the panel, so the transport pill never covers a control. The rack scrolls sideways when the window is narrower than its cards. There is no scrollbar. A card is the plain card of the design system with 16 px padding. No rack ears, screws or gradients. Today the rack holds one card, the instrument of the track. A slot that is empty or whose tool has no view shows a card with the tool name and one muted line.
- Mixer section, September 20, 2026: the right end of the track panel row, after the rack and outside what scrolls, so it is in the same place whatever a track holds. One hairline at `alpha/5` parts it from the rack, then 24 px of air, the title `Mixer` like a card title, and one row: the Gain knob, the Pan knob and the Mute button. The knobs are the 44 px knobs in 64 px columns of the synth card, with their labels and readouts in the same places: `0 dB`, `-6 dB`, and `C`, `50L`, `100R` for the pan. Mute is a 28 px button in the row of the knobs, with no label under it because it says what it is: subtle when the track plays, peach at 10% when it is muted. No meter, no fader, no solo.
- Selected track: its header gets an `alpha/5` fill in the shape and the place of a clip, 8 px from the edges of the header column. No accent, because it is a fill.
- Synth card: the title `Synth` at 14 px medium in `gray-900`, then one row of controls. The waveform is a segmented control, then seven 44 px knobs in 64 px columns. Air makes the groups, 32 px between them and 8 px inside: oscillator, filter (cutoff, resonance), envelope (attack, decay, sustain, release), output (gain). No boxes and no group captions: the labels already say what a group is. Under each knob its label at 12 px in `gray-700` and its value at 12 px in `gray-950` with tabular numbers: `480 Hz`, `2 kHz`, `5 ms`, `1.5 s`, `40%`. Three significant digits at most, no zeros at the end, so a value at rest is short.
- Focus: the arrangement and the note editor each show a 1 px lavender ring inside their edge, only when the focus came from the keyboard. A knob shows it as a 2 px lavender ring around its face, because 1 px on a 35 px circle was too weak to find, and a segmented control as its 1 px border, under the same rule. Tab goes from the project menu to the arrangement, then the note editor and its close icon, or the close icon of the track panel and its controls from left to right, then the transport.
- Cursor: a left-right resize cursor over the edges of a clip and over the end of a note, 6 px wide or a quarter of a narrow shape. Nothing else changes on hover.
- Scroll pans, pinch or cmd-scroll zooms in time about the pointer. A click on a ruler seeks to the nearest sixteenth. The rest is under "Using the app".

## Using the app

`cargo run -p runtime -- <project-folder>` opens the window. Everything snaps to a sixteenth. Every drag and every key below is one undo step, and escape during a drag puts it back.

| Where | Mouse or key | What it does |
| --- | --- | --- |
| Anywhere | space | Play or pause |
| Anywhere | cmd-z, shift-cmd-z | Undo, redo. Both wait while a drag is going on |
| Anywhere | tab, shift-tab | Move the focus: project menu, arrangement, the panel below, transport |
| Project menu | Add track | A new track with a synth |
| Project menu | Open terminal in project folder | The macOS Terminal in the folder, to start a coding agent there |
| Ruler | click | Move the playhead there |
| Transport | drag the tempo up or down | Change the tempo at the playhead. Half a bpm per pixel, a tenth of that with shift. Escape during the drag puts it back |
| Transport | up or right, down or left on the focused tempo | One bpm. With shift a tenth |
| Transport | the metronome button | The click on or off. It is not an undo step and changes no file |
| Arrangement or note editor | scroll, cmd-scroll or pinch | Pan, zoom in time |
| Arrangement | double click on empty track space | Add a clip of one bar |
| Arrangement | click on a clip | Select it |
| Arrangement | drag a clip | Move it in time and to another track |
| Arrangement | drag the left or right edge of a clip | Resize it. The left edge stops at the first note |
| Arrangement | delete or backspace | Delete the selected clip |
| Arrangement | left, right, up, down | Move the selected clip by a sixteenth, or to the track above or below |
| Arrangement | double click on a clip, or enter | Open the note editor for it. It takes the place of the track panel |
| Arrangement | click on a track header | Select the track and open its track panel. It takes the place of the note editor |
| Arrangement | up, down, with a track and no clip selected | Select the track above or below. The open track panel follows |
| Arrangement | enter, with a track and no clip selected | Open the track panel |
| Arrangement or track panel | escape | Close the panel below |
| Track panel | the close icon | Close the panel |
| Track panel | drag a knob up or down | Change the value. The sound follows. Escape during the drag puts it back |
| Track panel | Mixer: Gain, Pan | The level of the track in decibels, and where it sits between the two channels |
| Track panel | Mixer: Mute | Silence the track, and click again to bring it back |
| Track panel | double click on a knob | Set its default |
| Track panel | up or right, down or left on a focused knob | One step, a fiftieth of the travel. With shift a five-hundredth |
| Track panel | click on a waveform, or left and right on the focused control | Switch the waveform |
| Note editor | drag on empty space inside the clip | Draw a note. It sounds |
| Note editor | click on a note | Select it. It sounds |
| Note editor | drag a note | Move it in time and pitch. A new pitch sounds |
| Note editor | drag the end of a note | Change its length |
| Note editor | delete or backspace | Delete the selected note |
| Note editor | left, right | Move the selected note by a sixteenth |
| Note editor | up, down, with shift | Move it by a semitone, by an octave |
| Note editor | click on a key of the strip | Hear that pitch |
| Note editor | escape or the close icon | Close the editor |

## GPUI notes from the first port, September 14, 2026

The UI SDK lives in `crates/ui`; the gallery in `crates/gallery` shows every component (`GALLERY_SECTION=foundation|inputs|overlays|composed cargo run -p gallery`; `cargo test -p gallery --test snapshots` renders PNGs without opening a window). GPUI 0.2.2 limits that shaped the components. GPUI is now pinned to Zed v1.20.2, where some of these no longer apply, as noted:

- No CSS transitions. Hover and active states swap instantly. Only the switch thumb and the working indicator animate, through `with_animation`.
- No focus-visible. Focus rings show on mouse focus too. Stateless components take an optional `FocusHandle` to show a ring. The pinned version has `.focus_visible(..)`. The dropdown menu trigger and the seek strip use it; the button does not yet. The knob and the segmented control keep their own focus handle in element state and show the ring only for a focus from the keyboard (`sound_ui::KeyboardFocus`).
- No built-in text widget. `text_input.rs` implements shaping, cursor, selection and IME itself. It is single-line; `.lines(n)` only makes the box taller. Real multi-line editing is future work.
- Key bindings are registered by the component on first use, scoped to a key context. The arrangement and the note editor use key listeners on their focused root and no bindings. The window binds a few globally: space, cmd-z, shift-cmd-z, tab, shift-tab, cmd-q. A global binding wins over a focused button, so space always toggles playback and enter activates the focused control.
- Draggable controls use drag events with a delta, so a plain click on a slider track does not jump the handle. The knob is different since September 20, 2026: it is controlled, as a control on saved state has to be. The caller gives the value on every render and hears a change. The knob works a drag out from the value at the press with its own mouse listeners, so a drag goes on outside the knob and a press without a move reports nothing.
- SVG icons take an explicit colour; `Icon` reads the inherited text colour at render time. Colour buttons therefore tint rather than invert on hover.
- Overlays anchor to a zero-size box on the trigger edge and snap to the window with a margin. Side is explicit, not collision-aware. Click-outside uses `on_mouse_down_out` with an occluding surface.
- No arc primitive; the knob draws its value ring with dots.
- No accessibility tree in 0.2.2. The pinned version has AccessKit support (roles, labels, actions; see `crates/gpui/examples/a11y.rs` in Zed). The components do not use it yet.
