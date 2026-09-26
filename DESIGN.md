# Design

Design decisions for the Sound Tools UI. Settled from the three web prototypes reviewed on September 10, 2026 (removed September 14; two screenshots remain in `docs/reference/`). The real UI SDK is built in GPUI in `crates/ui`; the gallery in `crates/gallery` shows every component and variant.

## Direction for the third milestone

**Decided by the owner on September 25, 2026:** our own palette (see "Colour"), the transport in the title row, and the mixer strip in the header column. The owner also asked for a shorter panel, one volume control on the meter, a power icon in the card header and a look of its own for each device. The sizes, components and devices below are this step's answer to that, drawn in the mockups. Where this section differs from the sections after it, this section holds.

Made on September 22, 2026, with milestone 3 step 0. The images are in `docs/reference/m3-step-0/`: `before/` is the window as built, at laptop size, `gallery/` the component gallery, and `mockups/` the direction. The mockups are drawn outside the product for review only. Step 1 builds the real thing and takes `before/` as its "before".

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

- Every device shows what it does to the sound. A card is a display of the sound on the left and a few knobs at its right. The display is not a picture only: its handles drag.
- One grid: every card is 192 pt tall, with two rows of 56 x 72 pt cells. The panel below the arrangement is 216 pt, close to a device view.
- A value is one bright line on dark: the arc and pointer of a knob, the thumb on a meter, a curve and its handles. Values are never coloured.
- Colour means something or is not there, see "Colour" below.
- The transport pill is the one floating shape, in the title row.
- A track's colour is on its marks only: its dot, its notes, its velocity bars.

It does not copy Ableton. The devices are Synth, Filter, Compressor, EQ, Reverb and Limiter with plain parameter names, the cards are the plain card of our design system, and the displays are our own drawings: no graphic, name or text of Ableton's.

Mockups, all in our palette: `mockups/window.png` (the whole window), `mockups/track-panel.png` (the mixer strip, Synth, Filter, Compressor), `mockups/track-panel-plugin-eq-reverb.png` (a plugin, EQ, Reverb expanded, solo on, the rack running past the edge), `mockups/master-panel.png` (the master and its Limiter), `mockups/note-editor-velocity.png`, and `mockups/components.png` (every control of step 1 with its states, the header icons, the volume on its meter, the grid).

### Type

Unchanged from the source design system: InterDisplay with `ss03` and `cv01`, tabular numbers for every number. In the rack: 14 medium for card titles, track names and buttons; 12 regular for labels, values and the line under a display; 12 medium in toggles and segments.

### Sizes

Checked against the 1470 x 920 window:

- Layout grid 8 pt, 4 pt inside controls. Radii 10 for a card, 8 for buttons, 6 for toggles, segments, selects, displays and clips, 3 for notes.
- Window, top to bottom: the title row, 48 pt, with the transport. The arrangement: a ruler of 32, track rows of 64, and a master row of 40 pinned at its bottom. The panel below. The header column is 176 pt wide.
- The track panel is **216 pt** tall: 12 above the cards, a card of 192, 12 below. It was 384. The arrangement then has 656 pt: the ruler, nine track rows and the master row.
- The note editor keeps 352 pt: a ruler of 32, 264 for 22 semitones and a velocity lane of 56. Notes need the room and devices do not, so the two no longer share one height, and opening one in place of the other moves the arrangement's lower edge. This changes "a swap between it and the note editor moves nothing" below.
- A card: a 32 pt header, a body of two rows of 72 pt, 16 pt below; 192 pt in all. It is 16 pt from each side to its body. The body is a display at the left, 8 pt of air, then two columns of cells.
- A cell is 56 x 72: a 36 pt knob at its top, the label line at 38 and the value line at 54, 14 pt each. A 24 pt toggle, segment or select sits 6 pt from the top of its cell, on the same line as a knob's centre. A control with no value leaves its value line empty. Empty cells are fine.
- A display is 118 pt tall, from the top of the body to 8 pt above the value line of row 2. That value line, under the display, carries its numbers or its scale.
- A card's width is 32 pt plus its display plus 8 plus 56 per column: 288 pt (Compressor) to 464 pt (EQ). A plugin card is 200 pt. Cards are 12 pt apart, and the rack starts 16 pt right of the header column.
- On the 1470 pt window the rack has 1262 pt. The track in `mockups/track-panel.png` (Synth, Filter, Compressor) takes 1016 of them. When the cards go past the right edge, a 48 pt fade to the window colour says so, and two-finger scroll moves the rack sideways.
- The header column of the track panel holds the track's name on the line of the card titles, the close icon at its right, and the mixer strip on the rows of the cards: the volume at the left from the top of row 1 to the value line of row 2, pan in row 1 at the right, M and S on the knob line of row 2.

### Components for step 1

Every one lives in `crates/ui` with a gallery entry. The synth, plugin cards, effect cards and the mixer strip use only these. Two-finger scroll over any of them pans the rack and never changes a value: a control that took the gesture would change a sound while the composer scrolls past.

Knob:

- 36 pt dial. A 270° track of 2.5 pt at `alpha/10`, the value arc on it in `gray-950` with round ends, a 21 pt face in `gray-300`, a 2 pt pointer in `gray-950`. A bipolar knob (pan, EQ gain) draws its arc from the top. Disabled at 40 %. Label in `gray-800`, value in `gray-950`.
- Drag up or down, 200 pt for the whole travel, from the value at the press. With shift ten times finer. Double click, or backspace on the focused knob, sets the default. Arrows step a fiftieth, with shift a five-hundredth. Escape during a drag puts it back. The cursor is the up-down resize cursor.
- Focus from the keyboard: a 2 pt lavender ring around the face.
- GPUI's `PathBuilder` has `arc_to` and `stroke` in the pinned version, so the arc no longer needs dots.

Volume, one control:

- The meter is the track of the fader. Two bars of 5 pt, 2 pt apart; the thumb, a 28 x 6 pt bar in `gray-950` with a 1.5 pt ring of the window colour, sits across them at the gain. The level shows through above and below the thumb. A tick at 0 dB. -inf to +6 dB, 0 dB at 80 % of the height, the same scale for thumb and level.
- Meter colours: green up to -6 dBFS, yellow to 0, a red clip light above the bars until it is clicked, a 1 pt peak line in `gray-950` held 1.5 s. It falls 20 dB a second. Nothing shows at rest.
- Drag the thumb, or anywhere on the meter: it moves from where it is, so a press never jumps. With shift ten times finer. Double click sets 0 dB. Arrows 0.5 dB, with shift 0.1 dB. The readout (`-3.5 dB`, `-inf`) sits on the value line under it.
- Tab reaches it as it reaches a knob, and the 2 pt lavender ring goes around the thumb when the focus came from the keyboard.
- The master meter: two 40 x 3 pt bars in the same colours at the right end of the transport pill.

Gain reduction: a bar of 4 to 6 pt from the top down in `gray-950`, 0 to 24 dB. It is not level, so it has no level colours. Compressor and Limiter draw it inside their display.

Toggle: 24 pt tall, 28 wide for a letter and wider for a word. Off: `alpha/5` with `gray-700` text. On: white at 10 %, or its colour at 16 % with that colour as text: mute peach, solo yellow. Click or space toggles. M and S of a track also sit in its arrangement header, right aligned, when they are on, when the pointer is on the header, or when it has the focus. A muted track has its name, dot and clips at 40 %.

Segmented and select: 24 pt, on the knob line of their cell or at the top of a display. A select is for a list that does not fit as segments, such as an EQ band shape.

Card header:

- 32 pt. The picker is the title, 16 pt from the left edge.
- At the right, 8 pt from the edge, icons in 24 pt targets 4 pt apart, every glyph 12 pt on the centre line of the title: **expand**, **power**, **close**. An instrument has only expand; the Limiter has expand and power; the other effects have all three.
- Power replaces the switch. On: the glyph in `gray-950`. Off: the glyph in `gray-600`, the title in `gray-700` and the body at 40 %; the sound passes through untouched. Pointer on an icon: `alpha/8` behind it.
- Expand shows the controls a card hides, in columns to the right of a hairline at `alpha/6`. The card gets wider and never taller. Whether a card is expanded is interface state and is not saved.

Display:

- An inset of `gray-50` at 70 % with 6 pt corners, 118 pt tall. A curve is 1.5 pt `gray-950` over a fill of `alpha/5`; grid lines at `alpha/4`, the 0 dB line at `alpha/8`. A level inside a display is green, because level is signal.
- Handles are 10 pt circles in `gray-950` with a ring of `gray-50`; a secondary handle is hollow. A handle drags, with shift ten times finer, and double click resets what it moves. Every value a handle moves also has a knob or a hidden control, which is the path for the keys: a display is never the only way to reach a value.
- The line under a display, on the value line of row 2, gives its numbers (`A 5 ms · D 350 ms · S 25% · R 120 ms`) or its scale (`100 · 1k · 10k`) in 12 pt.

Plugin card: the picker, the `Open window` button at the top of the body, and `CLAP · <maker>` on the value line of row 2. 200 pt wide.

### Settled when the components were built

Step 1a, September 25, 2026. What the spec above left open, or what the build showed. The components are in `crates/ui/src/components/`, and the rack and focus sections of the gallery show them in every state.

- Contrast: `gray-700` is under 4.5 : 1 on a card (4.27 : 1) and on `alpha/5` over the window. So the text of a toggle that is off, of a segment that is not picked, and the line under a display are `gray-800`, not `gray-700`. Labels are `gray-800` on a card (6.14 : 1). Muted text on the window stays `gray-700` (4.71 : 1).
- One gesture for the knob, the volume and the handles, in `gesture.rs`. Shift pressed or let go during a drag goes on from where the value is. Past an end, a drag back first undoes the overshoot, so there and back ends where it began. Backspace or delete on the focused control sets its default; for the volume that is 0 dB, as the double click.
- Knob: the pointer runs from 3.5 pt to 8 pt from the centre. The round ends of the arc and the pointer are circles over the ends, because the pinned GPUI does not export lyon's line caps; so only an opaque colour gets round ends, and the unlit track has square ones. The focus ring is 2 pt outside the face.
- A cell is its own component, `Cell`, and the knob is a cell. A label wider than 56 pt (`Resonance`) runs into the air next to it and is not cut.
- Volume scale: `0.8 · 10^(dB / 61.94)` of the height, one formula that gives -inf at 0, 0 dB at 80 % and +6 dB at the top. A drag gives tenths of a dB. The arrows go from `-inf` to -60 dB and from under -60 dB to `-inf`; the drag reaches everything between. The meter keeps 5 pt at its top for the clip light (3 pt, the width of both bars) and the scale starts under it. The value is in dB with `f32::NEG_INFINITY` at the bottom; how a record saves `-inf` is step 2's.
- Meter: the bars fall 20 dB a second and the peak line waits 1.5 s, in `meter::Ballistics`, which the owner feeds with one peak per frame. Under -96 dB a level is silence, so after the sound the bars and the peak line come to rest and nothing shows. Not a number is silence too, never the top of the scale. The empty bar is `alpha/6`. Above 0 dB the bar stays yellow; the clip light is the red.
- Master meter: `Meter::horizontal`, two 40 x 3 pt bars 2 pt apart that run to the right, with the clip light at their right end, 45 x 8 pt in all.
- Gain reduction: 5 pt wide by default, on an `alpha/6` track.
- Toggle: `Toggle` with an optional colour for on. A word gets 10 pt of padding on each side. Enter or space toggles it through GPUI's keyboard click; in the window, space plays first, as on any focused button.
- The select is not a component of its own: it is `DropdownMenu` with `Trigger::Select`, a 24 pt trigger that says what is picked.
- Device card: the 1 pt border is inside the width, so a card is exactly 32 + display + 8 + 56 per column wide. Power off puts the title at 60 % opacity rather than recolouring it, since the title is whatever element the owner gives, usually a picker with its own colour. The hidden columns sit after a hairline in 8 pt of air on each side. A header icon has a 1 pt lavender ring for a focus from the keyboard. The power glyph of a device that is off is `gray-700`, not `gray-600`: an icon needs 3 : 1 and `gray-600` has 2.85 : 1 on a card, `gray-700` 4.27 : 1.
- Ids: a device card scopes what its controls keep under its id, and a display keys the state of a handle under its own id, so two cards with controls of one name, such as two filters, keep a focus and a drag each.
- Keys: a control takes the keys it has, also at an end (an arrow on a knob at its end stays the knob's), and lets the others go. A handle has no keys but escape during its drag.
- Display: a handle moves one or two values, each an `Axis` on a `KnobRange` across the width or the height, or fixed. A handle whose place depends on another value, such as the decay corner of an envelope after its attack, gives its axis a range offset by that value. The target of a handle is 18 pt, larger than its 10 pt dot, for a trackpad. Controls at the top of a display sit 6 pt in from its edges.
- Scroll: none of these controls handles a scroll, so two-finger scroll passes to what holds them.

### Settled when the window was built

Step 1b, September 26, 2026. What the spec above left open, or what the build showed. The after images are in `docs/reference/m3-step-1b/`.

- The view of a device draws its whole card. The rack gives it a `CardFrame` (the picker as title, and the close icon of an effect), and the tool registers its card with `Views::register_card`. The view owns the body and whether it is expanded, so the card could not be split between the rack and the view.
- Power is left out until a device has a bypass: no device has one yet, and an icon that does nothing is noise. Decided by the orchestrator: bypass is saved on the effect slot of the track, not in each device record, so plugins and built-in effects share it. Step 2 builds it and adds `CardFrame::power`. The plugin card has no expand either, because it hides nothing. So a plugin effect has only close, and an instrument plugin no icon.
- S is left out until step 2 brings solo. M sits where the spec puts M and S, at the left of the pan column, so S goes right of it.
- The volume of a track edits `gain_db`, which keeps -60 to 6 dB. The bottom of the control, `-inf`, is -60 dB until step 2 decides how a record keeps silence, as the orchestrator decided. Its undo step is `Change volume`. Its meter and the master meter in the transport are at rest until step 2 feeds them.
- The synth envelope: each time (attack, decay, release) has a zone of 28 % of the width on the travel of its knob, the sustain is held for 10 %, full level is at 88 % of the height and silence at 8 %. So any time from 1 ms to 10 s shows and the handle moves as its knob turns. The corner is one handle for decay and sustain, `Change decay and sustain`. The waveform is two segments with words, `Saw` and `Square`, as in the gallery.
- The note editor is 352 pt now, its ruler included, and it opens with the middle of its notes in the middle of its area: nothing floats over it. The velocity lane of step 9 takes its lowest 56 pt. The arrangement has no room below its last track any more for the same reason.
- The transport is 36 pt, in the middle of the room right of the project menu, and has no shadow: it floats over nothing. In a narrow window the air around it goes first, so it never covers the menu. Record while it records is solid red; the click while it sounds is white with a dark glyph, and a muted glyph when it is off. Tab goes through the title row first, in reading order: the project menu, then the transport, then the arrangement and the panel below it.
- The window is at least 1100 x 640: the project menu with the whole transport of a fitted project beside it, and a track panel under a few track rows.
- The tempo and the steadiness are `DragNumber`s, on the gesture of every other control: shift is ten times finer, so the steadiness moves by tenths of a percent with shift (it was fifths). A drag moves in whole steps from the value it began on, and after shift is pressed or let go, in steps from where the value is then, so neither jumps.
- The notice has a width of 400 pt and a notice is as wide as its text up to that. With only a largest width GPUI measured the text on one line and painted it on three, past the window edge. The notices sit 24 pt in from the corner the main view leaves free: right of the track headers and above the panel below, whichever is open, so they never cover the mixer strip. The main view says where that corner is (`Session::set_notice_room`), because the window knows no arrangement. This changes "The notice stays bottom-left, 24 pt from the edges" above.
- A picker or menu item that is off says why in words: `This project does not load plugins.`, `This project does not load the synth.`, `This project does not include the tempo fit.` The file edit is in `agent-docs/project-json.md`, which every project gets whatever it enables: its example lists every extension of the runtime, and its text says to add the missing ones and reopen.
- A dropdown trigger gives way in a narrow place and ends its label in an ellipsis, so a long plugin name in a card title leaves room for the icons.
- Tab goes through a card column by column, not row by row as "What this changes in the window" says: that is the order of the elements, and a tab index per cell was not worth it.
- The real window opens at 1470 x 920, the size the design is for.

### Devices

Steps 5 to 8 build from these. "Shown" is on the card, "hidden" is behind expand. Every device keeps its parameters in its record, shown or hidden.

| Device | Display, what drags | Shown | Hidden |
| --- | --- | --- | --- |
| Synth, 352 pt | The envelope: attack, decay to sustain, release, as one line. The attack peak drags sideways, the decay corner sideways and up, the release end sideways. The waveform as two drawn segments at the top right of the display. The line under it: A, D, S, R. | Cutoff, Resonance, Gain | Attack, Decay, Sustain, Release as knobs |
| Filter, 352 pt | The response curve. One handle at the cutoff: sideways is cutoff, up and down is resonance. The type (Low, Band, High, Notch) as segments at the top of the display. The line under it: the frequency scale. | Cutoff, Resonance, Drive, Mix | Slope, LFO rate, LFO depth |
| Compressor, 288 pt | The transfer curve, input across and output up, with the knee. The handle at the threshold drags sideways; the line above it drags up and down for the ratio. The level now as a green dot on the curve, and gain reduction as a bar at the right edge. The line under it: the gain reduction now. | Threshold, Ratio, Attack, Release | Knee, Makeup, Mix, Lookahead |
| EQ, 464 pt | The summed curve with a numbered handle per band: sideways is frequency, up and down is gain. A click selects a band. The line under it: the frequency scale. | Frequency, Gain, Q and Shape of the selected band | Each band's on and off, output gain |
| Reverb, 352 pt | The decay in time: pre-delay, early reflections as thin lines, the tail as a straight line in dB that reaches the floor at the decay time, and the highs as a dashed line that falls sooner with more damping. The start drags sideways for pre-delay, the end for decay. The line under it: pre-delay and decay. | Size, Damping, Width, Mix | Low cut, High cut, Freeze, Diffusion, and Pre-delay and Decay as knobs |
| Limiter, 352 pt | The last four seconds of output peaks in green under the ceiling line, with gain reduction hanging from the top. The ceiling line drags up and down. The line under it: ceiling and gain reduction. | Gain, Ceiling, Release | Lookahead |

### What this changes in the window

Decided by the owner on September 25, 2026: the transport in the title row and the mixer strip in the header column.

- The transport moves into the title row, centred, 36 pt tall, with the same contents plus the master meter at its right end. Nothing floats over content any more, so the note editor and a velocity lane are free. See the Quiet rule.
- The mixer strip of a track is in the header column of the track panel, as in "Sizes". The rack gets the full width. This replaces "Mixer section, September 20, 2026".
- The master is a row of 40 pt pinned under the tracks, with a ring where a track has its dot. A click opens its panel: the master volume in the header column, and the rack with the Limiter first.
- The velocity lane is the lowest 56 pt of the note editor: a 3 pt bar at the start of each note in the track colour at 70 %, the selected one in `gray-950`. Drag a bar up or down. `Velocity` in 12 pt `gray-700` in the header column.
- The notice stays bottom-left, 24 pt from the edges, at most 400 pt wide. A message wraps to at most three lines and the third ends in an ellipsis. The box grows with its text and its bottom stays 24 pt above the window edge, so it always fits (item 3). The full text is in `problems.txt` for a file that is not live, and in the tooltip of the notice for an error.
- The picker and the menu say in words why an item is off and keep the file edit for the agent docs (item 4). For example: `This project does not load plugins.`
- Tab order in the track panel: the close icon of the panel, then the header column top to bottom and left to right (volume, pan, M, S), then the rack card by card: each card's header (picker, expand, power, close) and then its cells row by row, the hidden ones too when the card is expanded. Last `Add effect`. A display handle is not a tab stop; its knob is.

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

## Colour: our own palette over the source grey scale

Dark only for now. The source dark theme uses `gray-50` as the darkest background and `gray-950` as text; keep that meaning.

Our own palette since September 25, 2026, decided by the owner. It replaced Catppuccin Mocha: a public theme that many apps wear is not an identity, and its violet greys shifted how the track and meter colours read. The token names stay, so only the values changed, in `crates/ui/src/theme.rs` (`Theme::dark`), with step 1a of the third milestone. A test there checks the contrast of the text tokens on their surfaces.

| Token | Hex | Token | Hex |
| --- | --- | --- | --- |
| gray-50 | `#0c0d10` | blue | `#7aa7ff` |
| gray-100 | `#121317` | sapphire | `#5cc0e8` |
| gray-200 | `#1b1d23` | sky | `#74d3ea` |
| gray-300 | `#292c34` | teal | `#5fd4c4` |
| gray-400 | `#363943` | green | `#7ee0a0` |
| gray-500 | `#474b56` | yellow | `#f3d27a` |
| gray-600 | `#60646f` | peach | `#f7a26b` |
| gray-700 | `#7c808c` | red | `#f7657a` |
| gray-800 | `#989ca8` | maroon | `#f08a96` |
| gray-900 | `#b5b8c2` | mauve | `#b894ff` |
| gray-950 | `#e9ebef` | pink | `#f28fd0` |
| alpha | `#ffffff` | lavender | `#a9b1ff` |
| | | rosewater | `#f5d9d2` |
| | | flamingo | `#f0bcbc` |

Roles:

| Role | Token |
| --- | --- |
| Window | `gray-100` |
| Card | `gray-200`, 1 pt border `alpha/6` |
| Knob face | `gray-300` |
| Display inset | `gray-50` at 70 % |
| Values, names, value arcs, thumbs, curves | `gray-950` |
| Control labels on a card | `gray-800`, 6.1 : 1. `gray-700` was 4.4 : 1, under 4.5 : 1 for 12 pt text |
| Muted text on the window | `gray-700` |
| Unlit arc, empty meter | `alpha/10`, `alpha/6` |

Each colour has one meaning:

- Green: sound is moving. Play, a meter below -6 dBFS, a level inside a display.
- Yellow: a meter from -6 to 0 dBFS, and solo.
- Peach: warning, files not live, and mute.
- Red: record, the clip light of a meter, errors.
- Lavender: keyboard focus, and the agent.
- Track colours: dots, notes, velocity bars. No control uses a track colour.

A meter is the one place colour fills an area, because level is a signal. Nothing else is filled with colour except a toggle that is on (its colour at 16 %). Solid colour buttons need dark text (`gray-200`).

## Quiet rule

Every element must earn its keep. Reference feel: the source design system and Hive. Lots of air, colour only where it means something, calm type. Audio tools are usually crowded; we are not copying that.

- Agent sidebar: a turn is the composer's message and the agent's result text. While working, one line such as `Building Polyrhythm` with a slow pulse. After, a muted `Worked for 12 s` that expands on click to the history. A failed build is `Build failed` in red plus one short sentence. No tool-call rows, progress bars, timestamps per message, or explanatory prose about builds and playback.
- Composer: input, model name, send. Placeholder inside the input is the only hint.
- Transport: a pill centred in the title row, since September 25, 2026. Play/pause, stop, record, position, duration if the project has one, a hairline seek strip, the tempo, the click and the master meter. Build status and device selection are not in it; at most a small dot when a reload is pending.
- Chrome: project name top-left as a quiet menu holding add, undo/redo, output device and the project folder in the Finder or in a terminal. No legends, no zoom controls, no grid.
- Cards and panels: 16 px padding, no meta chips in headers, port labels on hover only, secondary parameters behind the expand icon of a card.
- Accessible: visible focus rings, labelled controls, full keyboard reach. This is a product requirement.

## The window, September 19, 2026

What is built, in `crates/runtime/src/window.rs`, `extensions/arrangement/src/view.rs` with `view/editor.rs` and `view/track_panel.rs`, `extensions/instrument/src/view.rs` and `extensions/plugin-host/src/view.rs`. The track panel was added on September 20, 2026. `cargo test -p runtime --test snapshots` renders it to PNGs.

- `cargo test -p runtime --test snapshots` renders the window at 1470 x 920 points since September 22, 2026, the laptop the design is for. It was 1440 x 900.
- One background, `gray-100`, for the whole window. No panels and no top bar: the title bar is transparent, the project name sits right of the traffic lights and the row around it drags the window.
- Transport pill: play or pause in green, stop, record in red, the position as `bar.beat`, the time as `m:ss` muted, then the hairline seek strip and the duration when the project has an end, then the tempo and the click. Numbers are tabular and the pill sizes from its content, so it stays still while playing and grows by a digit at bar 100 or at ten minutes. Space toggles playback. Tab reaches the buttons, the strip, the tempo and the click, and left and right seek by a bar on the strip. This changes with the third milestone, see "Direction for the third milestone".
- Tempo, September 20, 2026: the tempo in effect at the playhead as a plain number in `gray_950` with a muted 12 px `bpm` after it, no box and no fill. Up to three decimals with no zeros at the end, so `120`, `93.5`, `120.125`. Dragging up on the number makes it faster, half a bpm per pixel. It moves by whole bpm from the tempo it began on and does not round the result, so 93.5 goes to 94.5 and back to exactly 93.5; with shift it moves by tenths at a tenth of the speed. The arrows step by 1 bpm and with shift by 0.1. It shows a 1 px lavender border when the focus came from the keyboard, like the seek strip. There is no tempo lane and no way to add or remove a tempo change in the window: an edit changes the tempo change in effect at the playhead, and the rest is a file edit.
- Steadiness, September 21, 2026: right of the tempo, and only when the project has a fit, so a project that was never fitted has the pill it always had. The same shape as the tempo: a whole percent in tabular `gray_950` with a muted 12 px `steady` after it, no box and no fill. Dragging up makes it steadier, one percent per pixel, by whole percent from where it began, with shift by tenths at a tenth of the speed (fifths before step 1b of the third milestone). The arrows step by 5 and with shift by 1. It is in the transport and not on the clip because it is the same kind of thing as the tempo: one number about the time of the whole project.
- Fit tempo to take, September 21, 2026: the second item of the project menu, under `Add track`, at 40 % when the selected clip was not recorded, and at 40 % with why under it for a project whose `extensions` does not list `fit-tempo`: `This project does not include the tempo fit.` It showed the file edit until step 1b of the third milestone; that is in `agent-docs/project-json.md` now. It is in the project menu and not on the clip because a fit is about the whole project: it rewrites the tempo map every other part follows. One undo step named `Fit tempo`. There is no control for the first downbeat and none for half and double; those are a file edit, and `agent-docs/fit-tempo.md` says which field to change.
- Click, September 20, 2026: a 28 px icon button at the right end of the pill, the metronome icon. A muted glyph when it is off, white with a dark glyph when it sounds (a subtle fill before step 1b of the third milestone, which was hard to see): no colour, because the click is a reference and not part of the piece. No volume, no count-in and no sounds to choose from.
- Record, September 20, 2026: a 28 px icon button right of stop, a circle in red, a red ring while it is off and solid red while it records (a subtle red fill before step 1b of the third milestone, which was hard to see). Red because a record control is red everywhere, and it is the one place a warning colour says something true. `r` toggles it, anywhere but in a text field. Pressing it starts playback if the project is stopped, because a take needs the playhead to move; pressing it again ends the take and leaves playback as it is. A stop, a pause or a seek ends the take too. The take goes to the track it began on, and so does the keyboard while it runs. There is no arm button per track, no count-in and no input meter: the take goes to the selected track, or to the first track when nothing is selected, so a keyboard always sounds somewhere.
- Project menu: add track, undo and redo with the name of the step and their shortcuts, the output device by name with a check, reveal project folder, open terminal in project folder. An item that cannot run is at 40% opacity.
- Arrangement: 176 px track headers with the accent dot and the name at 14 px medium in `gray-900`, 64 px rows, a 32 px ruler with one short mark and one 12 px number in `gray-700` per bar. Bar numbers thin out to every 2nd, 4th, 8th bar when bars get narrow. No grid lines, no row lines, no zoom or scroll controls. Two hairlines at `alpha/5`: under the ruler and right of the headers. Tick 0 sits 8 px into the timeline.
- Clips: `alpha/5` fill with an `alpha/10` hairline border and 6 px corners, 4 px inside the row. The notes are small bars in the accent of the track. That is the one place where a track accent is more than a dot: notes are marks, not fills, and they tie a clip to its track without a label. The selected clip has a `gray-950` border. Clips have no name label.
- Playhead: a 1 px `gray-950` line with a 7 px round head in the ruler.
- The view follows the playhead, September 20, 2026: while the project plays, the arrangement pages forward once the playhead passes the right edge, and the playhead lands back at the left edge. A stop or a seek that leaves the playhead off screen brings it back the same way. While the composer has scrolled the playhead off screen nothing pulls the view back, until the next stop or seek. There is no follow switch, because there is nothing to switch off. The note editor does not follow: it shows one clip.
- Notices: quiet lines bottom-left, 400 px wide at most. A red dot for the last error with a dismiss button that Tab reaches, a peach dot for files that are not live. A long message wraps to at most three lines. Nothing blocks.
- Note editor: a panel of 352 px below the arrangement (384 before step 1b of the third milestone), on the same background, with one hairline above it. No toolbar and no tools: what the pointer is on decides what a drag does. The header column lines up with the track headers: the accent dot and the name of the track in the ruler row, then a quiet close icon at 60% opacity, and below them a slim key strip of 32 px at the right edge, white keys at `alpha/10` and black keys at `alpha/3`, with only the Cs named in 12 px `gray-700` left of it. Rows are 12 px per semitone. The rows of black keys are tinted `alpha/2`, so a pitch can be read without lines between rows. Bar lines are hairlines at `alpha/5`, beat lines at half of that and only from 24 px per beat. This is the one place with a grid, because notes are placed by it. The arrangement keeps none. Outside the clip the area is a shade darker (`gray-50` at 50%). Notes are rounded bars of 11 px in the track accent with 3 px corners, the selected one filled with `gray-950`, the lightest colour there is, inside its accent outline. An outline alone on a pastel fill was hard to see. The ruler and the playhead are those of the arrangement. The editor opens zoomed to fit its clip, with the middle of its notes in the middle of its area.
- Track panel: the other thing the panel below the arrangement can show, in the same 384 px, so a swap between it and the note editor moves nothing. One at a time, like the clip view and the device view of Ableton. The header is that of the note editor: the accent dot, the name of the track and the quiet close icon, in the same places. Right of the header column is the rack: device cards from left to right, 24 px from the edges, at the top of the panel, so the transport pill never covers a control. The rack scrolls sideways when the window is narrower than its cards. There is no scrollbar. A card is the plain card of the design system with 16 px padding. No rack ears, screws or gradients. The rack holds the instrument of the track first, then its effects in the order the sound goes through them, then the control that adds one. This changes with the third milestone, see "Direction for the third milestone".
- Instrument picker, September 20, 2026: the first row of every device card is a quiet dropdown menu with no border and no fill, whose label is the name of what is in the slot and which opens the list of what else could go there. It is the card's title as well, so no card has a title of its own. It sits where a card title sits: 8 px of card padding above it and 8 px left of it, because the trigger brings its own. The menu is 280 px wide with one group, `Instrument`, that scrolls past 320 px: `Synth` first, then every CLAP and VST 3 instrument of this Mac with `CLAP · <maker>` or `VST 3 · <maker>` as a muted second line, and, while a scan is still running or whenever a VST 3 plugin is offered, a quiet note under the list: "Still looking for the plugins of this Mac…" and Steinberg's trademark notice. What is already in the slot has a check. An instrument this project cannot load is at 40 % and cannot be picked, with why under it: `This project does not load plugins.` The file edit is for the agent docs since step 1b of the third milestone. A second line wraps rather than being cut, so it can be read. There is no search box and no favourites; a list of a few dozen is read, not searched. Picking one is one undo step, named `Choose <name>`; picking the one that is already there does nothing.
- Effects in the rack, September 21, 2026: an effect is a card like any other, after the instrument, with the same picker as its title and one quiet 24 px close icon at 60 % right of that name, which takes the effect off the track. Its picker offers what this Mac declares an effect, in a group called `Effect`, and nothing else on the card changes: a plugin effect shows `Open window` like a plugin instrument. An effect whose plugin this Mac does not have is named by its id with the same muted line, and the sound passes through that card to the next one, so a missing effect is a card to fix and not a track that went quiet.
- Add effect, September 21, 2026: at the end of the rack, after the last card and outside any card, a quiet dropdown menu that says `Add effect` and lists the same offers. It sits where a card title sits, so the row of names reads across the rack. Picking one puts that effect at the end of the chain, as one undo step named `Add <name>`; the close icon of a card removes one, as one step named `Remove <name>`. There is no way to reorder with the mouse: the order is the `effects` list of the track record, which an agent or a file edit writes.
- Plugin card: the picker with the plugin's name, then one 28 px subtle button, `Open window` or `Close window`. Nothing else: a plugin's knobs are the plugin's own, in its own window. A plugin without a window of its own has the button at 40 % with one muted line under it, and a plugin that did not load has one line and no button. A VST 3 plugin is offered the button until it is asked once and turns out to have no window, which it then says in the quiet line bottom-left; asking a VST 3 plugin before that means building its whole interface, which is up to a second. A plugin this Mac does not have is named by its id in the picker, with one muted line saying so: the record stays as it is and the track is silent, and the quiet line bottom-left points at `problems.txt`. Since step 1b of the third milestone the card is the 200 pt device card, see "Settled when the window was built".
- The plugin's window, September 20, 2026, for both formats since September 21: a window of its own beside the main one, with a normal title bar called `<plugin> — <project>`, as big as the plugin asks. A plugin that asks for another size gets it. Nothing of ours is drawn in it. Since September 26, step 4b of the third milestone: it floats above the main window and hides while another application is in front; the composer can drag its edge when the plugin allows it, and the window ends on the size the plugin takes; and it comes back where it was, open or closed, when the project opens again. A window that comes back by itself leaves the keyboard with the main window.
- Mixer section, September 20, 2026: the right end of the track panel row, after the rack and outside what scrolls, so it is in the same place whatever a track holds. One hairline at `alpha/5` parts it from the rack, then 24 px of air, the title `Mixer` like a card title, and one row: the Gain knob, the Pan knob and the Mute button. The knobs are the knobs of the synth card, with their labels and readouts in the same places: `0 dB`, `-6 dB`, and `C`, `50L`, `100R` for the pan, whose arc starts at the top. Mute is a toggle in a cell of the row of the knobs, with no label under it because it says what it is: peach at 16 % when it is muted. Since step 1a of the third milestone the knobs are the 36 pt knobs in 56 pt cells. No meter, no fader, no solo. This changes with the third milestone, see "Direction for the third milestone".
- Selected track: its header gets an `alpha/5` fill in the shape and the place of a clip, 8 px from the edges of the header column. No accent, because it is a fill.
- Synth card: the picker says `Synth`, then one row of controls. The waveform is a segmented control, then seven knobs, the 36 pt knob in a 56 pt cell since step 1a of the third milestone. Air makes the groups, 32 px between them and 8 px inside: oscillator, filter (cutoff, resonance), envelope (attack, decay, sustain, release), output (gain). No boxes and no group captions: the labels already say what a group is. Under each knob its label at 12 px in `gray-700` and its value at 12 px in `gray-950` with tabular numbers: `480 Hz`, `2 kHz`, `5 ms`, `1.5 s`, `40%`. Three significant digits at most, no zeros at the end, so a value at rest is short.
- Focus: the arrangement and the note editor each show a 1 px lavender ring inside their edge, only when the focus came from the keyboard. A knob shows it as a 2 px lavender ring around its face, because 1 px on a 35 px circle was too weak to find, and a segmented control as its 1 px border, under the same rule. Tab goes from the project menu to the arrangement, then the note editor and its close icon, or the close icon of the track panel and its controls from left to right, which begins with the picker of the first card and ends with the control that adds an effect, then the transport: play, stop, record, the seek strip, the tempo and the click. Since step 1b of the third milestone the transport comes right after the project menu, see "Settled when the window was built".
- Cursor: a left-right resize cursor over the edges of a clip and over the end of a note, 6 px wide or a quarter of a narrow shape. Nothing else changes on hover.
- Scroll pans, pinch or cmd-scroll zooms in time about the pointer. A click on a ruler seeks to the nearest sixteenth. The rest is under "Using the app".

## Using the app

`cargo run -p runtime -- <project-folder>` opens the window. Everything snaps to a sixteenth. Every drag and every key below is one undo step, and escape during a drag puts it back.

| Where | Mouse or key | What it does |
| --- | --- | --- |
| Anywhere | space | Play or pause |
| Anywhere | r | Record on the selected track from the playhead, and again to end the take |
| Anywhere | cmd-z, shift-cmd-z | Undo, redo. Both wait while a drag is going on |
| Anywhere | tab, shift-tab | Move the focus: project menu, transport, arrangement, the panel below |
| Project menu | Add track | A new track with a synth |
| Project menu | Fit tempo to take | Fit the project tempo to the take of the selected clip. One undo step |
| Project menu | Open terminal in project folder | The macOS Terminal in the folder, to start a coding agent there |
| Ruler | click | Move the playhead there |
| Transport | drag the tempo up or down | Change the tempo at the playhead. Whole bpm from where it began, half a bpm per pixel. With shift tenths, at a tenth of the speed. Escape during the drag puts it back |
| Transport | up or right, down or left on the focused tempo | One bpm. With shift a tenth |
| Transport | drag the steadiness up or down | How steady the fitted tempo is, 0 as played to 100 one tempo. One percent per pixel, with shift tenths. Escape during the drag puts it back. It shows only when the project has a fit |
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
| Track panel | the volume, under the track name | The level of the track in decibels: drag the thumb or the meter, double click for 0 dB, arrows by 0.5 dB and with shift 0.1 dB |
| Track panel | Pan | Where the track sits between the two channels |
| Track panel | M | Silence the track, and click again to bring it back |
| Track panel | drag a handle of a display | The value it moves, as its knob does. With shift ten times finer, double click resets. Escape during the drag puts it back |
| Track panel | the expand icon of a card | Show the controls the card hides, such as the envelope knobs of the synth |
| Track panel | double click on a knob, or backspace on the focused knob | Set its default |
| Track panel | shift while dragging a knob | Ten times finer, from where the value is |
| Track panel | up or right, down or left on a focused knob | One step, a fiftieth of the travel. With shift a five-hundredth |
| Track panel | click on a waveform, or left and right on the focused control | Switch the waveform |
| Track panel | click the name on a card | Pick another instrument or effect for that card. One undo step |
| Track panel | Add effect, at the end of the rack | Put an effect at the end of the chain. One undo step |
| Track panel | the close icon in the header of an effect card | Take that effect off the track. One undo step, and undo brings it back as it sounded |
| Track panel | Open window, on the card of a plugin | The plugin's own window, above this one, where it was the last time. The same control closes it |
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

The UI SDK lives in `crates/ui`; the gallery in `crates/gallery` shows every component (`GALLERY_SECTION=foundation|rack|focus|overlays|composed cargo run -p gallery`; `cargo test -p gallery --test snapshots` renders PNGs without opening a window). GPUI 0.2.2 limits that shaped the components. GPUI is now pinned to Zed v1.20.2, where some of these no longer apply, as noted:

- No CSS transitions. Hover and active states swap instantly. Only the switch thumb and the working indicator animate, through `with_animation`.
- No focus-visible. Focus rings show on mouse focus too. Stateless components take an optional `FocusHandle` to show a ring. The pinned version has `.focus_visible(..)`. The dropdown menu trigger and the seek strip use it; the button does not yet. The knob and the segmented control keep their own focus handle in element state and show the ring only for a focus from the keyboard (`sound_ui::KeyboardFocus`).
- No built-in text widget. `text_input.rs` implements shaping, cursor, selection and IME itself. It is single-line; `.lines(n)` only makes the box taller. Real multi-line editing is future work.
- Key bindings are registered by the component on first use, scoped to a key context. The arrangement and the note editor use key listeners on their focused root and no bindings. The window binds a few globally: space, cmd-z, shift-cmd-z, tab, shift-tab, cmd-q. A global binding wins over a focused button, so space always toggles playback and enter activates the focused control.
- Draggable controls use drag events with a delta, so a plain click on a slider track does not jump the handle. The knob is different since September 20, 2026: it is controlled, as a control on saved state has to be. The caller gives the value on every render and hears a change. The knob works a drag out from the value at the press with its own mouse listeners, so a drag goes on outside the knob and a press without a move reports nothing.
- SVG icons take an explicit colour; `Icon` reads the inherited text colour at render time. Colour buttons therefore tint rather than invert on hover.
- Overlays anchor to a zero-size box on the trigger edge and snap to the window with a margin. Side is explicit, not collision-aware. Click-outside uses `on_mouse_down_out` with an occluding surface.
- No arc primitive in 0.2.2. The pinned version has `PathBuilder` with `arc_to` and `stroke`, and the knob, the display and `components/paint.rs` use them. It does not export lyon's line caps, so round ends are circles painted over the ends.
- No accessibility tree in 0.2.2. The pinned version has AccessKit support (roles, labels, actions; see `crates/gpui/examples/a11y.rs` in Zed). The components do not use it yet.
