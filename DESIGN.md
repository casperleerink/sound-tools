# Design

Design decisions for the Sound Tools UI. Settled from the three web prototypes reviewed on September 10, 2026 (removed September 14; two screenshots remain in `docs/reference/`). The real UI SDK is built in GPUI in `crates/ui`; the gallery in `crates/gallery` shows every component and variant.

## Direction for the third milestone

**Proposed, waiting for the owner's approval.** Until then the sections after this one hold. What this section changes in them is named where it does.

Made on September 22, 2026, with milestone 3 step 0. The images are in `docs/reference/m3-step-0/`: `before/` is the window as built, at laptop size, `gallery/` the component gallery, and `mockups/` this proposal. The mockups are drawn outside the product for review only. Step 1 builds the real thing and takes `before/` as its "before".

The target is a 13 to 14 inch MacBook with a trackpad and keys: the window is 1470 x 920 points at scale 2. `cargo test -p runtime --test snapshots` renders it at that size since this step.

### What is wrong now

Located in `before/`. Measured in points.

Window:

1. The transport pill floats over content. In the note editor it covers the lowest keys and notes (`editor.png`), and a velocity lane at the bottom would sit under it. In the arrangement it covers the last rows when there are many tracks (`scale.png`).
2. The title row, 48 pt tall and the full width, holds only the project name.
3. The error notice runs off the bottom of the window: its third line is cut at the edge (`notices.png`). This is a bug in the notice layout.
4. The disabled items of the instrument picker and of the project menu show a file edit (`Add "plugin-host" to "extensions" in project.json...`) to a composer (`track-panel-picker-disabled.png`, `fit-action-disabled.png`).

Track panel:

5. Three title lines that do not line up: the card title `Synth`, `Add effect` and `Mixer` sit at three heights (`track-panel.png`).
6. The knob row of the mixer section is about 25 pt higher than the knob row of the synth, and `Mute` sits on neither the knob line nor the label line.
7. Card heights follow content: the synth card is 160 pt, a plugin card 105, a missing plugin 93. The bottoms of the rack are ragged (`track-panel-effects.png`, `track-panel-effect-missing.png`).
8. Card widths follow content. The synth card alone is 737 pt wide, so the synth and two effects do not fit next to the mixer section at 1470 pt. The third card hides behind the mixer section with no sign that the rack scrolls (`track-panel-synth-effects.png`).
9. The header column is 176 x 350 pt of empty space under the track name, and about 200 pt under the cards is empty too.
10. Four control styles in one row, with no shared cell: the waveform control has a label and no value line, the knobs have both, `Mute` is a text button, a plugin card has a 28 pt button. Groups inside the synth are made by air alone, and `Gain` sits alone at the far right.
11. The knob draws its value as 25 dots, lit and unlit. At a glance the value is hard to read, the pointer is a 3 pt dot, and the ring reads as texture.
12. The knob has no fine drag: 160 pt for the whole travel, so on the log range of the cutoff 1 pt is about 4 % in frequency. Shift makes only the arrow keys finer.
13. Two effects of one plugin are two cards with one name (`track-panel-effects.png`). The close icon of an effect sits 8 pt from its menu chevron, easy to miss on a trackpad.
14. Recording shows as a 10 % red fill behind an outlined circle, and the click as a subtle fill. Both are hard to see on a laptop screen (`transport-recording.png`, `transport-click-on.png`).

Design system:

15. No identity of our own. The palette is Catppuccin Mocha as published, the gallery shows web components nobody uses (badges, alerts, dialog), and the gallery's mixer strip has a third design for volume, a horizontal slider (`gallery/composed.png`).
16. Control labels are `gray-700` on a card, 4.4 : 1. That is under 4.5 : 1 for 12 pt text, and the Quiet rule makes accessibility a product requirement.
17. Track colours include green, yellow, peach and red, which also mean play, solo, warning and record. They are on dots and notes only, so this stays, but no control may use a track colour.

### The direction

Sound Tools looks like one calm instrument. What makes it ours:

- One grid for every control: a cell of 64 x 80 pt, three rows per card, every card the same height. A row is a group.
- A value is one bright line on dark: the arc and pointer of a knob, the line on a fader cap, the gain reduction bar, a curve. Values are never coloured.
- Colour means something or is not there. See "Colour" below.
- The transport pill is the one floating shape, in the title row.
- A track's colour is on its marks only: its dot, its notes, its velocity bars.

It does not copy Ableton. The effects are Filter, Compressor, EQ and Reverb with plain parameter names, the cards are the plain card of our design system, and no graphic, name or text of Ableton's is used.

Mockups: `mockups/window.png` (the whole window), `mockups/track-panel.png` (synth, Filter, Compressor, the mixer strip), `mockups/track-panel-plugin-eq-reverb.png` (plugin, EQ, Reverb, Filter, solo on), `mockups/master-panel.png` (master and limiter), `mockups/note-editor-velocity.png`, `mockups/components.png` (every control of step 1 with its states), and the same window in our own palette, `mockups/window-own-palette.png` and `mockups/track-panel-own-palette.png`.

### Colour

Roles, whichever palette the owner picks:

| Role | Token |
| --- | --- |
| Window | `gray-100` |
| Card | `gray-200`, 1 pt border `alpha/6` |
| Knob face | `gray-300` |
| Fader cap | `gray-400` |
| Display inset (curve, keys) | `gray-50` at 70 % |
| Values, names, value arcs | `gray-950` |
| Control labels on a card | `gray-800` (5.8 : 1). Was `gray-700` |
| Muted text on the window | `gray-700` |
| Unlit arc, fader track, empty meter | `alpha/10`, `alpha/10`, `alpha/5` |

Each colour has one meaning. This replaces "Accents are sparse" and the "one accent" of the Quiet rule, which a meter cannot keep:

- Green: sound is moving. Play, and a meter below -6 dBFS.
- Yellow: a meter from -6 to 0 dBFS, and solo.
- Peach: warning, files not live, and mute.
- Red: record, the clip light of a meter, errors.
- Lavender: keyboard focus, and the agent.
- Track colours: dots, notes, velocity bars.

A meter is the one place colour fills an area, because level is a signal. Nothing else is filled with colour except a toggle that is on (its colour at 16 %).

The palette is the owner's choice:

- A. Keep Catppuccin Mocha as it is.
- B. Our own palette, **recommended**. The same token names and roles, new hex values in one file (`crates/ui/src/theme.rs`). The reasons: Catppuccin is a public theme that many apps wear, so it is not an identity; its greys lean violet, which shifts how the track colours and meter colours read; ours are cooler and closer to neutral, with brighter text for more contrast.

| Token | B | Token | B |
| --- | --- | --- | --- |
| gray-50 | `#0c0d10` | blue | `#7aa7ff` |
| gray-100 | `#121317` | sapphire | `#5cc0e8` |
| gray-200 | `#1b1d23` | teal | `#5fd4c4` |
| gray-300 | `#292c34` | green | `#7ee0a0` |
| gray-400 | `#363943` | yellow | `#f3d27a` |
| gray-500 | `#474b56` | peach | `#f7a26b` |
| gray-600 | `#60646f` | red | `#f7657a` |
| gray-700 | `#7c808c` | mauve | `#b894ff` |
| gray-800 | `#989ca8` | lavender | `#a9b1ff` |
| gray-900 | `#b5b8c2` | pink | `#f28fd0` |
| gray-950 | `#e9ebef` | | |

The colours B does not list (sky, maroon, rosewater, flamingo) keep their Mocha values until a track needs them to change.

### Type

Unchanged from the source design system: InterDisplay with `ss03` and `cv01`, tabular numbers for every number. In the rack: 14 medium for card titles, track names and buttons; 12 regular for labels and values; 12 medium in toggles and segments.

### Sizes

- Layout grid 8 pt, 4 pt inside controls. Radii 10 for a card, 8 for segments, selects and buttons, 6 for toggles and clips, 3 for notes.
- Window: title row 48 with the transport, header column 176, ruler 32, track rows 64, a master row of 40 pinned under the tracks. The panel below is 352 pt for the track panel and the note editor alike. It was 384.
- Rack: cards 24 pt from the top of the panel and from the header column, 12 pt apart, top aligned. A card is 304 pt tall: a 44 pt header, three rows of 80, 20 below. It is 32 pt plus 64 per column wide, plus 40 for a gain reduction column. So cards differ only in how many columns they have.
- A cell is 64 x 80. The control sits on the knob line (a 40 pt knob at 4 pt from the top of the cell; 28 pt segments and selects and 24 pt toggles centred on the same line). The label line is at 46 and the value line at 62, 16 pt each. A control that needs no value, such as a segmented choice, leaves its value line empty. A display (a curve, the gain reduction bar) takes whole cells.
- One row is one group, read left to right and top to bottom: for the synth, oscillator, filter, envelope. No boxes and no group captions. Empty cells are fine.
- When the cards go past the right edge, a 48 pt fade to the window colour says so. Two-finger scroll moves the rack sideways, as now.

### Components for step 1

Every one lives in `crates/ui` with a gallery entry. The synth, plugin cards, effect cards and the mixer strip use only these.

Knob:

- 40 pt dial. A 270° track of 3 pt at `alpha/10`, the value arc on it in `gray-950` with round ends, a 24 pt face in `gray-300`, a 2 pt pointer in `gray-950` from 5 to 11 pt out. A bipolar knob (pan, EQ gain) draws its arc from the top. Disabled at 40 %.
- Drag up or down, 200 pt for the whole travel, from the value at the press. With shift ten times finer. Double click, or backspace on the focused knob, sets the default. Arrows step a fiftieth, with shift a five-hundredth. Escape during a drag puts it back. The cursor is the up-down resize cursor.
- No two-finger scroll on a knob: the same gesture pans the rack, and a knob that took it would change a sound while the composer scrolls past.
- Focus from the keyboard: a 2 pt lavender ring around the face.
- GPUI's `PathBuilder` has `arc_to` and `stroke` in the pinned version, so the arc no longer needs dots.

Fader:

- Vertical, from row 1 to the value line of row 3 of the rack. A 2 pt track at `alpha/10`, a tick at 0 dB, a 28 x 14 cap in `gray-400` with a 1 pt `gray-950` line. -inf to +6 dB, 0 dB at 80 % of the travel.
- The cap follows the finger one to one, with shift ten times finer. A press on the track does not jump. Double click sets 0 dB. Arrows 0.5 dB, with shift 0.1 dB. The readout (`-3.5 dB`, `-inf`) sits on the value line.

Meter:

- Two bars of 4 pt, 2 pt apart, on the scale of the fader beside it so 0 dB lines up. Green up to -6 dBFS, yellow to 0, and a red clip light above the bars that stays until it is clicked. A 1 pt peak line in `gray-950` that holds 1.5 s. It falls 20 dB a second. Nothing shows at rest.
- Gain reduction (compressor, limiter): one 6 pt bar from the top down in `gray-950`, 0 to 24 dB, labelled `GR`. It is not level, so it has no level colours.
- The master meter: the same colours as two 40 x 3 pt bars at the right end of the transport pill.

Toggle:

- 24 pt tall, 28 wide for a letter and 48 for a word. Off: `alpha/5` with `gray-700` text. On: white at 10 %, or its colour at 16 % with that colour as text: mute peach, solo yellow. Click or space toggles.
- M and S of a track sit in its header, right aligned, when they are on, when the pointer is on the header, or when it has the focus. A muted track has its name, dot and clips at 40 %.

Segmented and select: 28 pt, as the segmented control is now, on the knob line of their cell. A select is for a list that does not fit one cell as segments, such as an EQ band shape.

Device card and card header:

- The plain card, 304 pt tall as above. The header is the picker as the title, 16 pt from the left edge. An effect has, at the right, a 26 x 16 switch that turns it on and off, and the close icon 12 pt right of it in a 24 pt target. An instrument has neither. A card that is off shows its body at 40 %.
- A plugin card: the `Open window` button on the knob line of row 1, and `CLAP · <maker>` as a muted line at the bottom. Nothing else.

Display: an inset in `gray-50` at 70 % with 6 pt corners. A curve is 1.5 pt `gray-950` over a fill of `alpha/5`. EQ band handles are 16 pt circles with their number, the selected one filled `gray-950`. Drag a handle for frequency and gain, with shift finer. Row 3 holds the controls of the selected band, which is also the path for the keys.

### What this changes in the window

- The transport moves into the title row, centred, 36 pt tall, with the same contents plus the master meter at its right end. Nothing floats over content any more, so the note editor and a velocity lane are free. This changes "Transport: floating pill, bottom centre" in the Quiet rule.
- The mixer strip of a track moves into the header column of the track panel, under the name, on the rows of the cards: the fader and its meter at the left, pan on row 1 at the right, M and S on row 2. The rack gets the full width. This changes "Mixer section, September 20, 2026".
- The master is a row of 40 pt pinned under the tracks, with a ring where a track has its dot. A click opens its panel: the master fader and meter in the header column, and the rack with the limiter first.
- The velocity lane is the lowest 56 pt of the note editor: a 3 pt bar at the start of each note in the track colour at 70 %, the selected one in `gray-950`. Drag a bar up or down. `Velocity` in 12 pt `gray-700` in the header column.
- The notice stays bottom-left and must fit inside the window (item 3).
- The picker and the menu say in words why an item is off and keep the file edit for the agent docs (item 4). For example: `This project does not load plugins.`

Everything else in the sections below stays: the arrangement, clips, the note editor, menus and the picker.

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
- Transport: floating pill, bottom centre. Play/pause, stop, record, position, duration if the project has one, a hairline seek strip, the tempo, and the click. Build status and device selection are not in it; at most a small dot when a reload is pending.
- Chrome: project name top-left as a quiet menu holding add, undo/redo, output device and the project folder in the Finder or in a terminal. No legends, no zoom controls, no grid.
- Cards and panels: 16 px padding, no meta chips in headers, port labels on hover only, secondary parameters behind a disclosure or a second view.
- Accessible: visible focus rings, labelled controls, full keyboard reach. This is a product requirement.

## The window, September 19, 2026

What is built, in `crates/runtime/src/window.rs`, `extensions/arrangement/src/view.rs` with `view/editor.rs` and `view/track_panel.rs`, `extensions/instrument/src/view.rs` and `extensions/plugin-host/src/view.rs`. The track panel was added on September 20, 2026. `cargo test -p runtime --test snapshots` renders it to PNGs.

- `cargo test -p runtime --test snapshots` renders the window at 1470 x 920 points since September 22, 2026, the laptop the design is for. It was 1440 x 900.
- One background, `gray-100`, for the whole window. No panels and no top bar: the title bar is transparent, the project name sits right of the traffic lights and the row around it drags the window.
- Transport pill: play or pause in green, stop, record in red, the position as `bar.beat`, the time as `m:ss` muted, then the hairline seek strip and the duration when the project has an end, then the tempo and the click. Numbers are tabular and the pill sizes from its content, so it stays still while playing and grows by a digit at bar 100 or at ten minutes. Space toggles playback. Tab reaches the buttons, the strip, the tempo and the click, and left and right seek by a bar on the strip.
- Tempo, September 20, 2026: the tempo in effect at the playhead as a plain number in `gray_950` with a muted 12 px `bpm` after it, no box and no fill. Up to three decimals with no zeros at the end, so `120`, `93.5`, `120.125`. Dragging up on the number makes it faster, half a bpm per pixel. It moves by whole bpm from the tempo it began on and does not round the result, so 93.5 goes to 94.5 and back to exactly 93.5; with shift it moves by tenths at a tenth of the speed. The arrows step by 1 bpm and with shift by 0.1. It shows a 1 px lavender border when the focus came from the keyboard, like the seek strip. There is no tempo lane and no way to add or remove a tempo change in the window: an edit changes the tempo change in effect at the playhead, and the rest is a file edit.
- Steadiness, September 21, 2026: right of the tempo, and only when the project has a fit, so a project that was never fitted has the pill it always had. The same shape as the tempo: a whole percent in tabular `gray_950` with a muted 12 px `steady` after it, no box and no fill. Dragging up makes it steadier, one percent per pixel, by whole percent from where it began, with shift by fifths at a fifth of the speed. The arrows step by 5 and with shift by 1. It is in the transport and not on the clip because it is the same kind of thing as the tempo: one number about the time of the whole project.
- Fit tempo to take, September 21, 2026: the second item of the project menu, under `Add track`, at 40 % when the selected clip was not recorded, and at 40 % with the one edit under it for a project whose `extensions` does not list `fit-tempo`, which is the line an instrument picker gives an offer such a project cannot take. It is in the project menu and not on the clip because a fit is about the whole project: it rewrites the tempo map every other part follows. One undo step named `Fit tempo`. There is no control for the first downbeat and none for half and double; those are a file edit, and `agent-docs/fit-tempo.md` says which field to change.
- Click, September 20, 2026: a 28 px icon button at the right end of the pill, the metronome icon. Ghost when it is off, subtle when it sounds, like the mute button of a track: no accent, because the click is a reference and not part of the piece. No volume, no count-in and no sounds to choose from.
- Record, September 20, 2026: a 28 px icon button right of stop, a circle in red, in the two variants play has: ghost while it is off, a subtle red fill while it records. Red because a record control is red everywhere, and it is the one place a warning colour says something true. `r` toggles it, anywhere but in a text field. Pressing it starts playback if the project is stopped, because a take needs the playhead to move; pressing it again ends the take and leaves playback as it is. A stop, a pause or a seek ends the take too. The take goes to the track it began on, and so does the keyboard while it runs. There is no arm button per track, no count-in and no input meter: the take goes to the selected track, or to the first track when nothing is selected, so a keyboard always sounds somewhere.
- Project menu: add track, undo and redo with the name of the step and their shortcuts, the output device by name with a check, reveal project folder, open terminal in project folder. An item that cannot run is at 40% opacity.
- Arrangement: 176 px track headers with the accent dot and the name at 14 px medium in `gray-900`, 64 px rows, a 32 px ruler with one short mark and one 12 px number in `gray-700` per bar. Bar numbers thin out to every 2nd, 4th, 8th bar when bars get narrow. No grid lines, no row lines, no zoom or scroll controls. Two hairlines at `alpha/5`: under the ruler and right of the headers. Tick 0 sits 8 px into the timeline.
- Clips: `alpha/5` fill with an `alpha/10` hairline border and 6 px corners, 4 px inside the row. The notes are small bars in the accent of the track. That is the one place where a track accent is more than a dot: notes are marks, not fills, and they tie a clip to its track without a label. The selected clip has a `gray-950` border. Clips have no name label.
- Playhead: a 1 px `gray-950` line with a 7 px round head in the ruler.
- The view follows the playhead, September 20, 2026: while the project plays, the arrangement pages forward once the playhead passes the right edge, and the playhead lands back at the left edge. A stop or a seek that leaves the playhead off screen brings it back the same way. While the composer has scrolled the playhead off screen nothing pulls the view back, until the next stop or seek. There is no follow switch, because there is nothing to switch off. The note editor does not follow: it shows one clip.
- Notices: quiet lines bottom-left, 400 px wide at most. A red dot for the last error with a dismiss button that Tab reaches, a peach dot for files that are not live. A long message wraps to at most three lines. Nothing blocks.
- Note editor: a panel of 384 px below the arrangement, on the same background, with one hairline above it. No toolbar and no tools: what the pointer is on decides what a drag does. The header column lines up with the track headers: the accent dot and the name of the track in the ruler row, then a quiet close icon at 60% opacity, and below them a slim key strip of 32 px at the right edge, white keys at `alpha/10` and black keys at `alpha/3`, with only the Cs named in 12 px `gray-700` left of it. Rows are 12 px per semitone. The rows of black keys are tinted `alpha/2`, so a pitch can be read without lines between rows. Bar lines are hairlines at `alpha/5`, beat lines at half of that and only from 24 px per beat. This is the one place with a grid, because notes are placed by it. The arrangement keeps none. Outside the clip the area is a shade darker (`gray-50` at 50%). Notes are rounded bars of 11 px in the track accent with 3 px corners, the selected one filled with `gray-950`, the lightest colour there is, inside its accent outline. An outline alone on a pastel fill was hard to see. The ruler and the playhead are those of the arrangement. The editor opens zoomed to fit its clip, with the middle of its notes in the middle of what the transport leaves free.
- Track panel: the other thing the panel below the arrangement can show, in the same 384 px, so a swap between it and the note editor moves nothing. One at a time, like the clip view and the device view of Ableton. The header is that of the note editor: the accent dot, the name of the track and the quiet close icon, in the same places. Right of the header column is the rack: device cards from left to right, 24 px from the edges, at the top of the panel, so the transport pill never covers a control. The rack scrolls sideways when the window is narrower than its cards. There is no scrollbar. A card is the plain card of the design system with 16 px padding. No rack ears, screws or gradients. The rack holds the instrument of the track first, then its effects in the order the sound goes through them, then the control that adds one.
- Instrument picker, September 20, 2026: the first row of every device card is a quiet dropdown menu with no border and no fill, whose label is the name of what is in the slot and which opens the list of what else could go there. It is the card's title as well, so no card has a title of its own. It sits where a card title sits: 8 px of card padding above it and 8 px left of it, because the trigger brings its own. The menu is 280 px wide with one group, `Instrument`, that scrolls past 320 px: `Synth` first, then every CLAP and VST 3 instrument of this Mac with `CLAP · <maker>` or `VST 3 · <maker>` as a muted second line, and, while a scan is still running or whenever a VST 3 plugin is offered, a quiet note under the list: "Still looking for the plugins of this Mac…" and Steinberg's trademark notice. What is already in the slot has a check. An instrument this project cannot load is at 40 % and cannot be picked, with the one edit under it: `Add "plugin-host" to "extensions" in project.json and open the project again.` A second line wraps rather than being cut, so it can be read. There is no search box and no favourites; a list of a few dozen is read, not searched. Picking one is one undo step, named `Choose <name>`; picking the one that is already there does nothing.
- Effects in the rack, September 21, 2026: an effect is a card like any other, after the instrument, with the same picker as its title and one quiet 24 px close icon at 60 % right of that name, which takes the effect off the track. Its picker offers what this Mac declares an effect, in a group called `Effect`, and nothing else on the card changes: a plugin effect shows `Open window` like a plugin instrument. An effect whose plugin this Mac does not have is named by its id with the same muted line, and the sound passes through that card to the next one, so a missing effect is a card to fix and not a track that went quiet.
- Add effect, September 21, 2026: at the end of the rack, after the last card and outside any card, a quiet dropdown menu that says `Add effect` and lists the same offers. It sits where a card title sits, so the row of names reads across the rack. Picking one puts that effect at the end of the chain, as one undo step named `Add <name>`; the close icon of a card removes one, as one step named `Remove <name>`. There is no way to reorder with the mouse: the order is the `effects` list of the track record, which an agent or a file edit writes.
- Plugin card: the picker with the plugin's name, then one 28 px subtle button, `Open window` or `Close window`. Nothing else: a plugin's knobs are the plugin's own, in its own window. A plugin without a window of its own has the button at 40 % with one muted line under it, and a plugin that did not load has one line and no button. A VST 3 plugin is offered the button until it is asked once and turns out to have no window, which it then says in the quiet line bottom-left; asking a VST 3 plugin before that means building its whole interface, which is up to a second. A plugin this Mac does not have is named by its id in the picker, with one muted line saying so: the record stays as it is and the track is silent, and the quiet line bottom-left points at `problems.txt`.
- The plugin's window, September 20, 2026, for both formats since September 21: a window of its own beside the main one, with a normal title bar called `<plugin> — <project>`, as big as the plugin asks and not resizable by dragging. A plugin that asks for another size gets it. Nothing of ours is drawn in it. It is not kept above the main window, so it can go behind it; cmd-` brings it back.
- Mixer section, September 20, 2026: the right end of the track panel row, after the rack and outside what scrolls, so it is in the same place whatever a track holds. One hairline at `alpha/5` parts it from the rack, then 24 px of air, the title `Mixer` like a card title, and one row: the Gain knob, the Pan knob and the Mute button. The knobs are the 44 px knobs in 64 px columns of the synth card, with their labels and readouts in the same places: `0 dB`, `-6 dB`, and `C`, `50L`, `100R` for the pan. Mute is a 28 px button in the row of the knobs, with no label under it because it says what it is: subtle when the track plays, peach at 10% when it is muted. No meter, no fader, no solo.
- Selected track: its header gets an `alpha/5` fill in the shape and the place of a clip, 8 px from the edges of the header column. No accent, because it is a fill.
- Synth card: the picker says `Synth`, then one row of controls. The waveform is a segmented control, then seven 44 px knobs in 64 px columns. Air makes the groups, 32 px between them and 8 px inside: oscillator, filter (cutoff, resonance), envelope (attack, decay, sustain, release), output (gain). No boxes and no group captions: the labels already say what a group is. Under each knob its label at 12 px in `gray-700` and its value at 12 px in `gray-950` with tabular numbers: `480 Hz`, `2 kHz`, `5 ms`, `1.5 s`, `40%`. Three significant digits at most, no zeros at the end, so a value at rest is short.
- Focus: the arrangement and the note editor each show a 1 px lavender ring inside their edge, only when the focus came from the keyboard. A knob shows it as a 2 px lavender ring around its face, because 1 px on a 35 px circle was too weak to find, and a segmented control as its 1 px border, under the same rule. Tab goes from the project menu to the arrangement, then the note editor and its close icon, or the close icon of the track panel and its controls from left to right, which begins with the picker of the first card and ends with the control that adds an effect, then the transport: play, stop, record, the seek strip, the tempo and the click.
- Cursor: a left-right resize cursor over the edges of a clip and over the end of a note, 6 px wide or a quarter of a narrow shape. Nothing else changes on hover.
- Scroll pans, pinch or cmd-scroll zooms in time about the pointer. A click on a ruler seeks to the nearest sixteenth. The rest is under "Using the app".

## Using the app

`cargo run -p runtime -- <project-folder>` opens the window. Everything snaps to a sixteenth. Every drag and every key below is one undo step, and escape during a drag puts it back.

| Where | Mouse or key | What it does |
| --- | --- | --- |
| Anywhere | space | Play or pause |
| Anywhere | r | Record on the selected track from the playhead, and again to end the take |
| Anywhere | cmd-z, shift-cmd-z | Undo, redo. Both wait while a drag is going on |
| Anywhere | tab, shift-tab | Move the focus: project menu, arrangement, the panel below, transport |
| Project menu | Add track | A new track with a synth |
| Project menu | Fit tempo to take | Fit the project tempo to the take of the selected clip. One undo step |
| Project menu | Open terminal in project folder | The macOS Terminal in the folder, to start a coding agent there |
| Ruler | click | Move the playhead there |
| Transport | drag the tempo up or down | Change the tempo at the playhead. Whole bpm from where it began, half a bpm per pixel. With shift tenths, at a tenth of the speed. Escape during the drag puts it back |
| Transport | up or right, down or left on the focused tempo | One bpm. With shift a tenth |
| Transport | drag the steadiness up or down | How steady the fitted tempo is, 0 as played to 100 one tempo. One percent per pixel, with shift fifths. Escape during the drag puts it back. It shows only when the project has a fit |
| Transport | up or right, down or left on the focused steadiness | Five percent. With shift one |
| Transport | the metronome button | The click on or off. It is not an undo step and changes no file |
| Transport | the record button | The same as `r` |
| A MIDI keyboard | any key, and the sustain pedal | Plays the instrument of the selected track, whether the project plays or not |
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
| Track panel | click the name on a card | Pick another instrument or effect for that card. One undo step |
| Track panel | Add effect, at the end of the rack | Put an effect at the end of the chain. One undo step |
| Track panel | the close icon on an effect card | Take that effect off the track. One undo step, and undo brings it back as it sounded |
| Track panel | Open window, on the card of a plugin | The plugin's own window, beside this one. The same control closes it |
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
- No arc primitive; the knob draws its value ring with dots. The pinned version has `PathBuilder` with `arc_to` and `stroke`.
- No accessibility tree in 0.2.2. The pinned version has AccessKit support (roles, labels, actions; see `crates/gpui/examples/a11y.rs` in Zed). The components do not use it yet.
