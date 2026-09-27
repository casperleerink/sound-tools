# Design

Design decisions for the Sound Tools UI. Settled from the three web prototypes reviewed on September 10, 2026 (removed September 14; two screenshots remain in `docs/reference/`). The real UI SDK is built in GPUI in `crates/ui`; the gallery in `crates/gallery` shows every component and variant.

## Direction for the fourth milestone

**Decided by the owner on September 26, 2026:** this direction, an arm toggle on audio tracks, and the newest clip covering an older one where they overlap. Made the same day, with milestone 4 step 0. It stays inside the language of the third milestone below: palette B, the 56 x 72 grid, cards of 192 pt, a display with handles and the main knobs, the rest behind expand. No new colour; the new uses of existing colours are listed under "Colour". The mockups are in `docs/reference/m4-step-0/mockups/`, drawn outside the product for review only:

- `window-audio.png`: the window with two audio tracks, a selected clip with fades and gain, and the track panel of an audio track.
- `recording.png`: two armed tracks recording over clips, takes growing to the playhead.
- `drop-file.png`: a file from the Finder over the space under the last track, and the Drum pad in the panel.
- `audio-clip.png`: every state of an audio clip, the audio track header, the Clip card, and dropping on the arrangement.
- `sampler.png` and `drum-pad.png`: the two instruments in every state.

### Audio tracks and clips

- `Add track` in the project menu offers `Instrument track` and `Audio track`. A new audio track is empty.
- An audio clip is the clip shape of a note clip: 56 pt in a 64 pt row, `alpha/5` fill, `alpha/10` border, 6 pt corners, no name label. Its waveform is a mark in the track colour at 85 %, mirrored about the middle, at most 22 pt each way. It is drawn as it sounds: scaled by the clip gain and faded.
- The pointer on a clip, or a selected clip, shows three handles at its top: a fade handle at each top corner, 7 pt down, and the gain handle, hollow, in the middle. A fade is a 1.5 pt `gray-950` line from the bottom corner to its handle, with `gray-50` at 50 % outside it. The gain handle drags up and down from where it is, and alt-up and alt-down change the gain of the selected clips by 1 dB. While a fade or the gain is dragged, its value shows on the label of a tempo mark: `Fade in 420 ms`, `-6 dB`.
- Clip edges never click. Every clip start and end gets a short fixed ramp that is not drawn and is not a setting. A fade of 0 ms means only that ramp.
- Trim is the clip edge of today, with its cursor. While an edge is dragged, the part of the file the clip hides shows past the edge at 25 %.
- A take while it records has a red border, and its right edge is the playhead. A take lies over the clips under it and changes none of them. A clip whose file is missing keeps its place and says `voice-take-2.wav is missing`.
- Header of an audio track in the arrangement: the dot, the name from 44 pt, and the arm toggle at 140 to 168, always there. It takes the place the third milestone planned for M and S; that plan is dropped, and M and S stay in the track panel. The toggle is what tells an audio track from an instrument track. Armed, it is red at 16 %, and the input level shows at 88 to 133 on the component of the master meter, 45 x 8. The name then has 44 to 80 and a long one ends in an ellipsis.
- Track panel of an audio track: the mixer strip of any track, and the input select at 84 to 144, under M (84 to 112) and S (116 to 144), ending on the value line of row 2. Its list is the channels of the macOS default input, each alone, then pairs. The meter of an armed track shows its input.
- The rack of an audio track starts with the **Clip card**, where an instrument track has its instrument. Decided: it comes in step 1. It shows the selected clip of the track. The title is the file name, plain. The display is the whole file in the waveform display: the start and end lines trim, the gain line has a fade at each end, and a green line is the playhead while it is inside the clip. The line under it says which part of the file plays: `2.1 s to 10.1 s of 14.6 s`. Shown: Gain, Fade in, Fade out. Hidden: Start and End, the keyboard path of the trim. It is 464 pt, 537 expanded. With no clip selected, it is the 200 pt card of an empty slot: `Select a clip of this track.`
- A full-height 1 pt hairline (`alpha/6`, as in an expanded card) stands between the Clip card and the first effect, with 12 pt of air on each side. The Clip card acts on one clip and the effects on the whole track, so the rack reads as two groups. No label and no colour. The rack of an instrument track has no divider, because its instrument acts on the whole track.

### Sampler, 464 pt

| Display, what drags | Shown | Hidden |
| --- | --- | --- |
| The waveform display of the sample. Start and end lines drag sideways. The envelope drawn over it, in the time of the sample from the start: the attack peak drags sideways, the decay corner sideways and up for the sustain. A green line where the last note is. The line under it: the file name, A, D, S. | Root, Velocity, Release, Gain | Start, End, Attack, Decay, Sustain |

The release comes after the key is let go, which is no place in the sample, so it has a knob and no handle. The root uses the note names of the app, where note 60 is C4, and starts on C4. Empty, the display says `Drop an audio file here` over a `Choose file` button, which opens the macOS file panel and is the keyboard path. Expanded, the card is 649 pt.

### Drum pad, 452 pt

- The display is the pad grid: 4 x 4 pads of 72 x 32, 4 pt apart, 300 x 140. So 16 pads fit the 192 pt card with no change: the grid runs from the top of the body to the bottom of the value line of row 2, the same height as a display and its line.
- Pads are notes 36 to 51, from the bottom left, row by row. The default kit follows the General MIDI drum map, so a drum part from elsewhere plays the right sounds: Kick, Rim, Snare, Clap, Snare 2, Tom 1, Hat, Tom 2, Pedal hat, Tom 3, Open hat, Tom 4, Tom 5, Crash, Tom 6, Ride. Hat, Pedal hat and Open hat start in the choke group.
- Shown: Volume, Pitch, Decay and Pan of the selected pad. Hidden: `Sound`, a select two cells wide, and `Choke`, a toggle. The list of `Sound` holds the synthesized sounds, the sample of the pad when it has one, and last `Choose file…`, the keyboard path to a sample pad. Expanded, the card is 581 pt.
- A click selects a pad and plays it. Tab reaches the grid as one stop. The arrows move the selection, and enter plays the selected pad.

### Dropping a file

- On the arrangement: a ghost of the clip the drop will make, from the snap step under the pointer and as long as the file, with the file name and a 2 pt lavender ring. Under the last track, the ghost comes with `New audio track` in the header column, and the new track takes the name of the file. Over an instrument track there is no target and the cursor says no. Several files go one after another on one track.
- On a card: the 2 pt lavender ring on the Sampler's display, or on the pad that takes the file. The drop makes it a sample pad.

### Recording

Decided by the owner: an arm toggle on audio tracks only. To set a level before a take, the composer must see the input without recording, and armed is that state. `r` records every armed audio track, plus the selected track when it is an instrument track, so a keyboard still always sounds somewhere. With nothing armed and an audio track selected, `r` arms that track and records.

### Overlapping clips on an audio track

Decided by the owner: where clips of an audio track overlap, the newest covers the older one, as in Ableton and Logic. No clip is changed, so moving or deleting the top clip brings the older one back. A retake then replaces what it lies over, and does not sound doubled. Note clips keep their rule: overlapping note clips all play.

### Colour

The meanings of "Colour" stay. These are the new uses, each an exception written down:

- Green fills a pad while it sounds, at 24 % fading with the sound: sound is moving. A green line in the waveform display is where the sound plays.
- The waveform of an audio clip is in the track colour: it is a mark, like a note, though it covers more area than notes do.
- Lavender is also the ring of a drop target, the ring a dragged effect card already shows on the card whose place it would take.
- Red is the arm toggle when on and the border of a take while it records.

### New components

- Waveform display: the display inset. The waveform is `alpha/30`, and the part outside start and end is shaded with `gray-50` at 72 %. Start and end are 1 pt `gray-950` lines with hollow handles 8 pt above the bottom. A curve over it (an envelope, or gain and fades) follows the rules of every display. The Sampler and the Clip card share it.
- Clip waveform: the mark in the track colour described above, with the three handles.
- Pad: 72 x 32, 6 pt corners, with the fill and border of a clip and its name in 12 pt medium, 8 pt in. Selected has the `gray-950` border. Sounding is green as above. A sample pad has a 12 pt waveform glyph at its right. A pad with no glyph is synthesized. A long name ends in an ellipsis.
- Arm toggle: the 28 x 24 toggle with a 10 pt circle glyph, red when on. The input select is the select of the design system.

### Grid arithmetic

- Window: title row 48, arrangement 656 (ruler 32, nine rows of 64, 8, master row 40), panel 216 (12, card 192, 12). 48 + 656 + 216 = 920.
- Card: 32 + 72 + 72 + 16 = 192. Width = 32 + display + 8 + 56 per column. Clip card and Sampler: 32 + 312 + 8 + 112 = 464. Drum pad: 32 + 300 + 8 + 112 = 452. Expanded adds 8 + 1 + 8 of air and hairline, plus 56 per hidden column: Clip card 464 + 17 + 56 = 537, Sampler 464 + 17 + 168 = 649, Drum pad 452 + 17 + 112 = 581.
- Pad grid: 4 x 72 + 3 x 4 = 300 wide, 4 x 32 + 3 x 4 = 140 tall, which is 72 + 54 + 14, the body top to the bottom of the value line of row 2.
- Racks at 1470: the divider of an audio track takes 12 + 1 + 12 = 25 where two cards had 12, so 13 more. `window-audio.png`, Clip card, divider, Compressor and Reverb, takes 464 + 25 + 288 + 12 + 352 = 1141 of 1262 pt. `recording.png`, the empty Clip card, divider, Compressor and Reverb, takes 200 + 25 + 288 + 12 + 352 = 877. `drop-file.png`, Drum pad and Compressor, takes 452 + 12 + 288 = 752.
- Header column, 176: the arm toggle at 140 to 168, 8 pt from the edge; the armed meter at 88 to 133; the name from 44, to 80 while armed. In the track panel, pan 84 to 140, M 84 to 112, S 116 to 144, and the input select 84 to 144, ending at 888 with the value line.

### Settled with audio clips in the window

Step 1b, September 26, 2026. What the direction left open, or what the build showed. The after images are in `docs/reference/m4-step-1b/`: `window/` from `cargo test -p runtime --test snapshots` (the `audio-*` states), `gallery/audio.png` from the gallery. Compared with the mockups side by side, the differences are listed last.

- The arm toggle is deferred to step 2, not dropped: the owner decided it stays, and recording brings arming. Until then it is left out, as power was left out until bypass, because a toggle that does nothing is noise. Step 2 built it, see "Settled with recording". The header of an audio track keeps its room, so its name ends at 132 pt, 8 pt before where the toggle goes. So until step 2 an audio track is told from an instrument track by its clips and its panel, not by its header.
- The waveform of a clip is one column of 1 pt per point, mirrored about the middle, the peak of what the column covers times the gain and the fades at that place, up to 22 pt each way, drawn as one shape through the middles of the columns. A quiet part is a hairline, so the middle line of the mockup shows through silence. While the overview of a file is made, or while nothing knows yet how long its file is, the clip shows its shape, one bar long when its trim does not say, and no waveform.
- The handles show on the clip under the pointer and on every selected clip. Their targets are 18 pt, and a clip narrower than 54 pt shows none: it keeps its body and edges to be pressed. The gain handle is pressed before the fade handles when they meet.
- The labels of a drag sit under the handle they belong to, into the clip: right of the fade in and the gain, left of the fade out. They are the label of a tempo mark: 20 pt, 6 pt corners, the window colour with an `alpha/10` border, 12 pt tabular text.
- The gain handle drags 72 dB, -48 to +24, over 200 pt, the travel of a knob, and so does the Gain knob of the Clip card. The gain line of the card's display uses the same range up the display, so 0 dB sits at two thirds of its height.
- A clip whose file is missing shows as long as its trim says, or one bar when it plays to the end of a file nobody can measure; its text ends in an ellipsis inside it.
- A muted track has its name, dot and clips at 40 %, note clips too: the design said it for every clip and it had not been built.
- The ghost of a drop is opaque, the window colour under the fill of a clip, since the clip it makes covers what it lies over. Its name is 14 pt medium, 12 pt in and 8 pt down, as in the mockup. Under the last track the header column shows a ring and `New audio track` in `gray-700`. Over an instrument track the cursor is the platform's "not allowed", and over a target its "copy".
- The Clip card's title is the file name in 14 pt medium, plain. Its second column has Fade out on the second row and nothing on the first, as drawn. The line under the display gives seconds with three digits: `2.1 s to 10.1 s of 14.6 s`. Its empty state and a clip whose file is missing are 200 pt cards with one quiet line. A drop of an effect card on the Clip card puts the effect first, as a drop on an instrument does.
- `Add track` is a group label in the project menu with `Instrument track` and `Audio track` under it, then a separator, then `Fit tempo to take`.
- Differences from the mockups, found side by side: no arm toggle yet (above, deferred to step 2); the voice and guitar of the snapshots are made by a formula, so their waveforms are more regular than the drawn ones; the window of `audio-window.png` has a track more than `window-audio.png` (the snapshot piece has no drums), and the meter above the volume of the voice shows the peak line of the snapshot's playback. Otherwise the clip shape, the handles, the fade lines, the labels, the Clip card with its divider, the empty card and the drop ghost match in place and size.

### Settled with recording

Step 2, September 26, 2026. The after images are in `docs/reference/m4-step-2/`: `window/` from `cargo test -p runtime --test snapshots` (`audio-armed`, `audio-input-select`, `audio-recording`), `gallery/` with the arm toggle (`rack.png`) and a take while it records (`audio.png`).

- The arm toggle is the toggle of M and S with a 10 pt circle for its face, 1 pt ring while off, filled while on, red at 16 % when on (`Toggle::dot`). It sits at 140 pt in the header of every audio track, as drawn, and is an element over the painted header, so tab reaches it.
- Armed, the meter of the input is the master meter, 45 x 8, at 88 pt, and the name ends at 80 pt with an ellipsis. The meter of the volume in the panel of an armed track shows the input, as drawn.
- The input select is the select of the design system at 84 pt, under M and S, its bottom on the value line of the second row. Its label is the channels: `In 1`, `In 1 + 2`. Before the input was first opened it offers two channels.
- A take while it records covers what it lies over, as the ghost of a drop does, since the clip it becomes covers: the window colour under the clip fill, a 1 pt red border, no handles. It starts where the recording began and its right edge is the playhead. Its waveform shows once the first frames are placed, a poll or two after the start.
- Differences from the mockups, side by side: the input select opens its list above itself when there is no room below, and its check is on the right, as in every menu of the app; the voice and guitar of the snapshots are made by a formula; the transport shows the record control filled red while it records, as in the second milestone. Otherwise the header, the meter, the toggle, the takes and the panel match in place and size.

### Settled with the Sampler

Step 3, September 26, 2026. The after images are in `docs/reference/m4-step-3/`: `window/sampler-*.png` from `cargo test -p runtime --test snapshots` (`WINDOW_SNAPSHOT_ONLY=sampler` renders them alone), `gallery/audio.png` from the gallery, its last block. Compared with `sampler.png` side by side, the differences are listed last.

- The envelope is drawn in the time of the file from the start line: silence at the foot of the start line, full level at 88 % of the height at the end of the attack, the sustain level from the end of the decay to the end line. Its handles move in the time of the file, so a handle and its knob agree to the millisecond; a short attack sits on the start line, as in the mockup.
- The attack peak and the decay corner are the full handles, the start and end the hollow ones of the waveform display. A drag of the decay corner is one step, "Change decay and sustain".
- Start and End keep 10 ms between them, as the trim of a clip. The end of the file is written as no end. With no file that plays they are dimmed.
- The Root knob moves in whole notes and says the note: `C4`, `C#4`. Its arrow keys step a semitone. The names are `Pitch::name` of `sound-notes`.
- Times read `2 ms`, `400 ms`, `1.18 s`; the gain `0 dB`, `-6 dB`.
- Empty, and with a file that is missing (`kalimba.wav is missing`) or does not play, the display says so in 12 pt `gray-800` over the subtle 28 pt `Choose file` button, 12 pt apart, in the middle of the display. The file panel says `Load`. The line under the display is left out then.
- The ring of a drop is the 2 pt lavender border of the display, over an opaque `gray-100` with the line in 12 pt `gray-950`: `Drop to load the file`, or `Drop to replace the file` over a Sampler with a file. It covers the waveform and the envelope, as the mockup covers the words of the empty display, and the handles hide while a file is over the display (`Display::takes_files`), since their dots reach 5 pt past it.
- A new file starts at its start: a drop or a choice sets `start_seconds` to 0 and leaves out `end_seconds`, and keeps the rest. A file under the name the record already names, which was missing, keeps the record whole, trims too.
- Differences from the mockup, found side by side: the kalimba of the snapshots is made by a formula, so its waveform is a smooth decay; the file icon and its name under the pointer while dragging are the platform's drag image, not ours. Otherwise the card, its display, the lines, the handles, the caption, the knobs, the expanded columns, the empty display and the ring match in place and size.

### Settled with the Drum pad

Step 4, September 26, 2026. The after images are in `docs/reference/m4-step-4/`: `window/` from `cargo test -p runtime --test snapshots` (the `drums-*` states), `gallery/drums.png` from the gallery. Compared with `drum-pad.png` and `drop-file.png` side by side.

- A pad whose sample file is missing has the `triangle-alert` glyph in peach where a sample pad has its waveform glyph: peach is "files not live". The mockups had no such state.
- The waveform glyph is lucide's `audio-lines`, 12 pt, in the text colour.
- The Sound select is 104 pt, two cells less 4 pt each side, its label at the left and its chevron at the right; its list is 200 pt and opens where the popover has room, above the card in the window. The pad's own file is a group of its own between the sounds and `Choose file…`.
- The value line under the Sound select says `Synthesized` or `Sample`; the Choke toggle says `On` or `Off`.
- The ring of the grid with the focus from the keyboard is the lavender 1 pt ring of every control, 3 pt outside the grid with 9 pt corners.
- A sounding pad is green at up to 24 %, in 24 steps of how loud it is against its hit, so a card whose pads hold still asks for no frame.
- A sample pad is named by its file name without the extension, lowercase as `assets/audio/` holds it: `shaker`, where the mockup wrote `Shaker`.
- Differences from the mockups, found side by side: the name of a sample pad (above); the meters of the Drums track are empty in the snapshot, which reads them on the timer of the real window. Otherwise the grid, the pads in every state, the knobs, the expanded card, the list and the drop ring match in place and size.

## Direction for the third milestone

**Decided by the owner on September 25, 2026:** our own palette (see "Colour"), the transport in the title row, and the mixer strip in the header column. The owner also asked for a shorter panel, one volume control on the meter, a power icon in the card header and a look of its own for each device. The sizes, components and devices below are this step's answer to that, drawn in the mockups. Where this section differs from the sections after it, this section holds.

Made on September 22, 2026, with milestone 3 step 0. The images are in `docs/reference/m3-step-0/`: `before/` is the window as built, at laptop size, `gallery/` the component gallery, and `mockups/` the direction. The mockups are drawn outside the product for review only. Steps 1a and 1b built the real thing and took `before/` as their "before".

The target is a 13 to 14 inch MacBook with a trackpad and keys: the window is 1470 x 920 points at scale 2. `cargo test -p runtime --test snapshots` renders it at that size since this step.

### What was wrong before step 1

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

Toggle: 24 pt tall, 28 wide for a letter and wider for a word. Off: `alpha/5` with `gray-700` text. On: white at 10 %, or its colour at 16 % with that colour as text: mute peach, solo yellow. Click or space toggles. M and S of a track were meant to sit in its arrangement header too; the fourth milestone drops that, because the arm toggle of an audio track takes that place, and M and S stay in the track panel. A muted track has its name, dot and clips at 40 %.

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
- Volume scale: `0.8 · 10^(dB / 61.94)` of the height, one formula that gives -inf at 0, 0 dB at 80 % and +6 dB at the top. A drag gives tenths of a dB. The arrows go from `-inf` to -60 dB and from under -60 dB to `-inf`; the drag reaches everything between. The meter keeps 5 pt at its top for the clip light (3 pt, the width of both bars) and the scale starts under it. The value is in dB with `f32::NEG_INFINITY` at the bottom; a record saves it as `"-inf"` since step 2.
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
- Power is left out until a device has a bypass: no device has one yet, and an icon that does nothing is noise. Decided by the orchestrator: bypass is saved on the effect slot of the track, not in each device record, so plugins and built-in effects share it. Step 2 built it with `CardFrame::power`: every effect card has power now. The plugin card has no expand either, because it hides nothing. So a plugin effect has only close, and an instrument plugin no icon.
- S is left out until step 2 brings solo. M sits where the spec puts M and S, at the left of the pan column, so S goes right of it. Step 2 put it there.
- The volume of a track edits `gain_db`. Its undo step is `Change volume`. Since step 2 the bottom of the control, `-inf`, is saved as `"-inf"` and is silence, and the meter under it and the master meter in the transport show real levels.
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
- The Filter, step 5 of the third milestone: the display spans 20 Hz to 20 kHz across and -36 to +18 dB up, so the +11.5 dB peak of full resonance fits. The handle sits on the peak of a low or high pass: its travel up and down is resonance stretched so that the dot is where the curve is, and it stops at 0 and 1. The slope is a segmented `12 · 24` with the label `Slope` and `dB / oct` on its value line; it is 8 pt wider than its cell and takes that from the air around it. The hidden columns are the slope, then LFO rate over LFO depth. Bypass is saved on the track's effect slot, and the rack gives the card its power icon since step 2.
- The Compressor, step 6 of the third milestone: the display is 136 pt, and the curve takes its left 118 pt, so it is square and a ratio of 1 is the diagonal; input and output are both -60 to 0 dBFS. The gain reduction bar is 5 pt, 6 pt in from the right edge and from top and bottom, right of the curve's end, so the hollow ratio handle at the end of the curve does not cover it. A line at the threshold. The level dot is 8 pt, green, and absent while nothing sounds. The line under the display is `GR -6.8 dB`, to a tenth. The ratio handle stops moving when the threshold is within 1 dB of 0 dBFS, where the line above it has no length; the Ratio knob still works. The hidden columns are Knee over Makeup, then Mix over Lookahead. Lookahead is a select of `0`, `1` and `10` with `ms` on its value line: as segments it is 75 pt wide and ran into the Makeup knob next to it.
- The EQ, step 7 of the third milestone: four bands. The display is 312 pt, 20 Hz to 20 kHz across and -18 to +18 dB up. A numbered handle is a 16 pt dot with its number in 10 pt medium; the selected band's dot is full, the others are rings, a band that is off is at 40 %. A cut or a notch has no gain, so its handle sits on the 0 dB line and moves only sideways, and the Gain knob is dimmed. The select of the shape shows an icon of the shape (`eq-*`, drawn like the lucide icons), with the band on the label line and the name of the shape on the value line: no name but `Bell` fits in a cell. The keys 1 to 4 select a band from any control of the card. The hidden columns are the on and off of bands 1 and 2 over those of 3 and 4, then Output.
- The Reverb, step 8 of the third milestone: pre-delay and decay each have a zone across on the travel of their knob, 14 % and 74 % of the width, with 4 % between so the shortest tail still falls. The tail is the curve, from full level at 88 % of the height to the floor, 60 dB down, at 8 %; its time is linear between its start and its end, so the dashed highs end at their part of the decay. The early reflections cannot be drawn in that time, where they would be one line: every other line is a thin mark under the tail in a zone after its start, 15 to 30 % of the width as the size grows. So the display is true about the pre-delay, the decay and the highs, and a picture of the reflections. The start is a hollow handle, the end a filled one. The display got two things for it: thin marks up from the bottom and a dashed line, 1 pt of `gray-950` at 50 %. Freeze is a toggle that says `Off` or `On` with the label `Freeze`. The power icon is the rack's; a bypassed reverb's tail stops at once. The hidden columns are Low cut over Diffusion, High cut over Pre-delay, Freeze over Decay: the top row as in the mockup.

### Settled with the mixer

Step 2, September 26, 2026. The after images are in `docs/reference/m3-step-2/`.

- The master row is drawn under the tracks and above the panel below, 40 pt, with a hairline above it. The playhead stops at its top edge, as in the mockup. Its header fill when its panel is open is the fill of a selected track header, and a focus from the keyboard shows a 1 pt lavender ring around that shape.
- The master panel has no "Add effect": effects on the master are not built, and a control that adds nothing is noise. The Limiter's title is plain text, not a picker, because nothing else can go in its place.
- The Limiter card is 352 pt: a display of 200 pt, Gain and Release in the first column, Ceiling in the second, Lookahead behind expand. The display's scale is the ceiling's, -24 to 0 dBFS from bottom to top. The ceiling is a line with its handle at the right end. Under it the last four seconds in 50 columns of 80 ms, green up to the loudest output of each, and the largest reduction of each as a thin `gray-950` bar hanging from the top, 24 dB for the whole height, a third of the width of a column so that it reads apart from the green it lies over. The line under the display: `Ceiling 0 dB · GR -4.1 dB`, the reduction of the last column.
- The default ceiling is 0 dB, not the -0.3 dB the mockup shows, so a project that never went over full scale renders as it did, see ARCHITECTURE.md.
- S is right of M, 4 pt apart, yellow when on. A power icon is on every effect card, between expand and close, and switches the bypass of its slot.
- The master meter at the right end of the transport shows the device output, and the meter of the master panel the output of the limiter. They differ by the click and anything connected to the device by hand.

### Settled with editing in the window

Step 9a, September 26, 2026. The images are in `docs/reference/m3-step-9a/window/`.

- The snap setting is a select in the corner above the track headers, on the line of the ruler: `Snap` in 12 pt `gray-700` at the left, 24 pt in like a track name, and the 24 pt select with what is picked at the right. It is interface state and not saved, like zoom: it is how the composer works and not part of the piece, and a setting in a file would be one more thing an agent could change under the composer. It starts on 1/16 in every session. Tab reaches it after the timeline.
- A tempo change after tick 0 is a mark in the ruler: a 1 pt `gray-700` line at its tick through the ruler, and a label of 20 pt with 6 pt corners, the number in `gray-950` and `bpm` in `gray-700`, on the window colour with an `alpha/10` border. On a bar line the label starts after the bar number, so the number stays; anywhere else it covers the bar numbers under it. The selected one has an `alpha/10` fill and a `gray-950` border, as a selected clip. The tempo at tick 0 has no mark: the transport shows it.
- There is no number to drag on a mark. A click on it moves the playhead onto it, and the tempo of the transport is the one drag number for the tempo there is. One control per value, and the ruler stays quiet.
- A rectangle on empty space is an `alpha/5` fill with an `alpha/20` border, 2 pt corners. Selected clips have the `gray-950` border of a selected clip.
- The name field is the 28 pt text input of the design system over the header of the track, its text where the painted name is.

Step 9b, September 26, 2026. The images are in `docs/reference/m3-step-9b/window/`.

- The velocity lane is as drawn above: a hairline over it, the bar lines and the shade outside the clip go through it, and `Velocity` sits in the header column. Bars of selected notes are painted after the others, so a selected note of a chord shows on top. The cursor over a bar is the up-down resize cursor.
- A rectangle in the note editor looks like the one of the timeline: an `alpha/5` fill with an `alpha/20` border, 2 pt corners. Selected notes are filled with `gray-950`, as the one selected note was.
- A double click on empty space adds a note, and a press on empty space starts a rectangle, as for clips. Drawing by a drag on empty space is gone; a drag of the second press of the double click draws the length.
- The header of an effect card is its grip. While it is dragged, its title rides under the pointer on a small card of 32 pt with the card colour at 90 %, and the card whose place it would take has a 2 pt lavender ring. The ring sits in the gap between cards, so nothing moves while dragging. The instrument card has no grip.
- With snap off, a new note and an arrow key are a sixteenth, not a thirty-second: a note added by a double click with snap off was too short to hit.

### Devices

Steps 5 to 8 built from these. "Shown" is on the card, "hidden" is behind expand. Every device keeps its parameters in its record, shown or hidden.

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

- Green: sound is moving. Play, a meter below -6 dBFS, a level inside a display. Since the fourth milestone: a sounding drum pad, and where the sound plays in a waveform display.
- Yellow: a meter from -6 to 0 dBFS, and solo.
- Peach: warning, files not live, and mute.
- Red: record, the clip light of a meter, errors. Since the fourth milestone: the arm toggle when on, and the border of a take while it records.
- Lavender: keyboard focus, and the agent. Also the ring where a drag lands: a dragged effect card, and, since the fourth milestone, a dropped file.
- Track colours: dots, notes, velocity bars, and since the fourth milestone the waveform of an audio clip. No control uses a track colour.

A meter is the one place colour fills an area, because level is a signal. Nothing else is filled with colour except a toggle that is on (its colour at 16 %). Solid colour buttons need dark text (`gray-200`).

## Quiet rule

Every element must earn its keep. Reference feel: the source design system and Hive. Lots of air, colour only where it means something, calm type. Audio tools are usually crowded; we are not copying that.

- Agent sidebar: a turn is the composer's message and the agent's result text. While working, one line such as `Building Polyrhythm` with a slow pulse. After, a muted `Worked for 12 s` that expands on click to the history. A failed build is `Build failed` in red plus one short sentence. No tool-call rows, progress bars, timestamps per message, or explanatory prose about builds and playback.
- Composer: input, model name, send. Placeholder inside the input is the only hint.
- Transport: a pill centred in the title row, since September 25, 2026. Play/pause, stop, record, position, duration if the project has one, a hairline seek strip, the tempo, the click and the master meter. Build status and device selection are not in it; at most a small dot when a reload is pending.
- Chrome: project name top-left as a quiet menu holding add, undo/redo, output device, another project, the project folder in the Finder or in a terminal, and the command line tool. No legends, no zoom controls, no grid.
- Cards and panels: 16 px padding, no meta chips in headers, port labels on hover only, secondary parameters behind the expand icon of a card.
- Accessible: visible focus rings, labelled controls, full keyboard reach. This is a product requirement.

## The window, September 19, 2026

What is built, in `crates/runtime/src/window.rs`, `extensions/arrangement/src/view.rs` with `view/editor.rs` and `view/track_panel.rs`, `extensions/instrument/src/view.rs` and `extensions/plugin-host/src/view.rs`. The track panel was added on September 20, 2026. `cargo test -p runtime --test snapshots` renders it to PNGs.

- `cargo test -p runtime --test snapshots` renders the window at 1470 x 920 points since September 22, 2026, the laptop the design is for. It was 1440 x 900.
- One background, `gray-100`, for the whole window. No panels and no top bar: the title bar is transparent, the project name sits right of the traffic lights and the row around it drags the window.
- Transport pill: play or pause in green, stop, record in red, the position as `bar.beat`, the time as `m:ss` muted, then the hairline seek strip and the duration when the project has an end, then the tempo and the click. Numbers are tabular and the pill sizes from its content, so it stays still while playing and grows by a digit at bar 100 or at ten minutes. Space toggles playback. Tab reaches the buttons, the strip, the tempo and the click, and left and right seek by a bar on the strip. This changes with the third milestone, see "Direction for the third milestone".
- Tempo, September 20, 2026: the tempo in effect at the playhead as a plain number in `gray_950` with a muted 12 px `bpm` after it, no box and no fill. Up to three decimals with no zeros at the end, so `120`, `93.5`, `120.125`. Dragging up on the number makes it faster, half a bpm per pixel. It moves by whole bpm from the tempo it began on and does not round the result, so 93.5 goes to 94.5 and back to exactly 93.5; with shift it moves by tenths at a tenth of the speed. The arrows step by 1 bpm and with shift by 0.1. It shows a 1 px lavender border when the focus came from the keyboard, like the seek strip. There is no tempo lane: an edit changes the tempo change in effect at the playhead. Since step 9a of the third milestone the ruler adds and removes tempo changes, see "Settled with editing in the window".
- Steadiness, September 21, 2026: right of the tempo, and only when the project has a fit, so a project that was never fitted has the pill it always had. The same shape as the tempo: a whole percent in tabular `gray_950` with a muted 12 px `steady` after it, no box and no fill. Dragging up makes it steadier, one percent per pixel, by whole percent from where it began, with shift by tenths at a tenth of the speed (fifths before step 1b of the third milestone). The arrows step by 5 and with shift by 1. It is in the transport and not on the clip because it is the same kind of thing as the tempo: one number about the time of the whole project.
- Fit tempo to take, September 21, 2026: the second item of the project menu, under `Add track`, at 40 % when the selected clip was not recorded, and at 40 % with why under it for a project whose `extensions` does not list `fit-tempo`: `This project does not include the tempo fit.` It showed the file edit until step 1b of the third milestone; that is in `agent-docs/project-json.md` now. It is in the project menu and not on the clip because a fit is about the whole project: it rewrites the tempo map every other part follows. One undo step named `Fit tempo`. There is no control for the first downbeat and none for half and double; those are a file edit, and `agent-docs/fit-tempo.md` says which field to change.
- Click, September 20, 2026: a 28 px icon button at the right end of the pill, the metronome icon. A muted glyph when it is off, white with a dark glyph when it sounds (a subtle fill before step 1b of the third milestone, which was hard to see): no colour, because the click is a reference and not part of the piece. No volume, no count-in and no sounds to choose from.
- Record, September 20, 2026: a 28 px icon button right of stop, a circle in red, a red ring while it is off and solid red while it records (a subtle red fill before step 1b of the third milestone, which was hard to see). Red because a record control is red everywhere, and it is the one place a warning colour says something true. `r` toggles it, anywhere but in a text field. Pressing it starts playback if the project is stopped, because a take needs the playhead to move; pressing it again ends the take and leaves playback as it is. A stop, a pause or a seek ends the take too. The take goes to the track it began on, and so does the keyboard while it runs. There is no count-in. Until the fourth milestone there was no arm button: the take went to the selected track, or to the first track when nothing is selected, so a keyboard always sounds somewhere. Since the fourth milestone, audio tracks have an arm toggle: `r` records every armed audio track, plus the selected track when it is an instrument track (or the first track when nothing is selected), and with nothing armed and an audio track selected it arms that track and records. The meter of an armed track shows its input.
- Project menu: add track, undo and redo with the name of the step and their shortcuts, the output device by name with a check, reveal project folder, open terminal in project folder. An item that cannot run is at 40% opacity.
- Arrangement: 176 px track headers with the accent dot and the name at 14 px medium in `gray-900`, 64 px rows, a 32 px ruler with one short mark and one 12 px number in `gray-700` per bar. Bar numbers thin out to every 2nd, 4th, 8th bar when bars get narrow. No grid lines, no row lines, no zoom or scroll controls. Two hairlines at `alpha/5`: under the ruler and right of the headers. Tick 0 sits 8 px into the timeline.
- Clips: `alpha/5` fill with an `alpha/10` hairline border and 6 px corners, 4 px inside the row. The notes are small bars in the accent of the track. That is the one place where a track accent is more than a dot: notes are marks, not fills, and they tie a clip to its track without a label. The selected clip has a `gray-950` border. Clips have no name label.
- Playhead: a 1 px `gray-950` line with a 7 px round head in the ruler.
- The view follows the playhead, September 20, 2026: while the project plays, the arrangement pages forward once the playhead passes the right edge, and the playhead lands back at the left edge. A stop or a seek that leaves the playhead off screen brings it back the same way. While the composer has scrolled the playhead off screen nothing pulls the view back, until the next stop or seek. There is no follow switch, because there is nothing to switch off. The note editor does not follow: it shows one clip.
- Notices: quiet lines bottom-left, 400 px wide at most. A red dot for the last error with a dismiss button that Tab reaches, a peach dot for files that are not live. A long message wraps to at most three lines. Nothing blocks.
- Note editor: a panel of 352 px below the arrangement (384 before step 1b of the third milestone), on the same background, with one hairline above it. No toolbar and no tools: what the pointer is on decides what a drag does. The header column lines up with the track headers: the accent dot and the name of the track in the ruler row, then a quiet close icon at 60% opacity, and below them a slim key strip of 32 px at the right edge, white keys at `alpha/10` and black keys at `alpha/3`, with only the Cs named in 12 px `gray-700` left of it. Rows are 12 px per semitone. The rows of black keys are tinted `alpha/2`, so a pitch can be read without lines between rows. Bar lines are hairlines at `alpha/5`, beat lines at half of that and only from 24 px per beat. This is the one place with a grid, because notes are placed by it. The arrangement keeps none. Outside the clip the area is a shade darker (`gray-50` at 50%). Notes are rounded bars of 11 px in the track accent with 3 px corners, the selected one filled with `gray-950`, the lightest colour there is, inside its accent outline. An outline alone on a pastel fill was hard to see. The ruler and the playhead are those of the arrangement. The editor opens zoomed to fit its clip, with the middle of its notes in the middle of its area.
- Track panel: the other thing the panel below the arrangement can show, in the same 384 px, so a swap between it and the note editor moves nothing. One at a time, like the clip view and the device view of Ableton. The header is that of the note editor: the accent dot, the name of the track and the quiet close icon, in the same places. Right of the header column is the rack: device cards from left to right, 24 px from the edges, at the top of the panel, so the transport pill never covers a control. The rack scrolls sideways when the window is narrower than its cards. There is no scrollbar. A card is the plain card of the design system with 16 px padding. No rack ears, screws or gradients. The rack holds the instrument of the track first, then its effects in the order the sound goes through them, then the control that adds one. Since step 1b of the third milestone the panel is laid out as "Settled when the window was built" says.
- Instrument picker, September 20, 2026: the first row of every device card is a quiet dropdown menu with no border and no fill, whose label is the name of what is in the slot and which opens the list of what else could go there. It is the card's title as well, so no card has a title of its own. It sits where a card title sits: 8 px of card padding above it and 8 px left of it, because the trigger brings its own. The menu is 280 px wide with one group, `Instrument`, that scrolls past 320 px: `Synth` first, then every CLAP and VST 3 instrument of this Mac with `CLAP · <maker>` or `VST 3 · <maker>` as a muted second line, and, while a scan is still running or whenever a VST 3 plugin is offered, a quiet note under the list: "Still looking for the plugins of this Mac…" and Steinberg's trademark notice. What is already in the slot has a check. An instrument this project cannot load is at 40 % and cannot be picked, with why under it: `This project does not load plugins.` The file edit is for the agent docs since step 1b of the third milestone. A second line wraps rather than being cut, so it can be read. There is no search box and no favourites; a list of a few dozen is read, not searched. Picking one is one undo step, named `Choose <name>`; picking the one that is already there does nothing.
- Effects in the rack, September 21, 2026: an effect is a card like any other, after the instrument, with the same picker as its title and one quiet 24 px close icon at 60 % right of that name, which takes the effect off the track. Its picker offers what this Mac declares an effect, in a group called `Effect`, and nothing else on the card changes: a plugin effect shows `Open window` like a plugin instrument. An effect whose plugin this Mac does not have is named by its id with the same muted line, and the sound passes through that card to the next one, so a missing effect is a card to fix and not a track that went quiet.
- Add effect, September 21, 2026: at the end of the rack, after the last card and outside any card, a quiet dropdown menu that says `Add effect` and lists the same offers. It sits where a card title sits, so the row of names reads across the rack. Picking one puts that effect at the end of the chain, as one undo step named `Add <name>`; the close icon of a card removes one, as one step named `Remove <name>`. The order is the `effects` list of the track record, which an agent or a file edit writes; since step 9b a drag of a card's header reorders it too.
- Plugin card: the picker with the plugin's name, then one 28 px subtle button, `Open window` or `Close window`. Nothing else: a plugin's knobs are the plugin's own, in its own window. A plugin without a window of its own has the button at 40 % with one muted line under it, and a plugin that did not load has one line and no button. A VST 3 plugin is offered the button until it is asked once and turns out to have no window, which it then says in the quiet line bottom-left; asking a VST 3 plugin before that means building its whole interface, which is up to a second. A plugin this Mac does not have is named by its id in the picker, with one muted line saying so: the record stays as it is and the track is silent, and the quiet line bottom-left points at `problems.txt`. Since step 1b of the third milestone the card is the 200 pt device card, see "Settled when the window was built".
- The plugin's window, September 20, 2026, for both formats since September 21: a window of its own beside the main one, with a normal title bar called `<plugin> — <project>`, as big as the plugin asks. A plugin that asks for another size gets it. Nothing of ours is drawn in it. Since September 26, step 4b of the third milestone: it floats above the main window and hides while another application is in front; the composer can drag its edge when the plugin allows it, and the window ends on the size the plugin takes; and it comes back where it was, open or closed, when the project opens again on the same Mac. A window that comes back by itself leaves the keyboard with the main window.
- Mixer section, September 20, 2026: the right end of the track panel row, after the rack and outside what scrolls, so it is in the same place whatever a track holds. One hairline at `alpha/5` parts it from the rack, then 24 px of air, the title `Mixer` like a card title, and one row: the Gain knob, the Pan knob and the Mute button. The knobs are the knobs of the synth card, with their labels and readouts in the same places: `0 dB`, `-6 dB`, and `C`, `50L`, `100R` for the pan, whose arc starts at the top. Mute is a toggle in a cell of the row of the knobs, with no label under it because it says what it is: peach at 16 % when it is muted. Since step 1a of the third milestone the knobs are the 36 pt knobs in 56 pt cells. No meter, no fader, no solo. Since step 1b of the third milestone the mixer is a strip in the header column, and since step 2 it has a fader, a meter and solo, see "Settled with the mixer".
- Selected track: its header gets an `alpha/5` fill in the shape and the place of a clip, 8 px from the edges of the header column. No accent, because it is a fill.
- Synth card: the picker says `Synth`, then one row of controls. The waveform is a segmented control, then seven knobs, the 36 pt knob in a 56 pt cell since step 1a of the third milestone. Air makes the groups, 32 px between them and 8 px inside: oscillator, filter (cutoff, resonance), envelope (attack, decay, sustain, release), output (gain). No boxes and no group captions: the labels already say what a group is. Under each knob its label at 12 px in `gray-700` and its value at 12 px in `gray-950` with tabular numbers: `480 Hz`, `2 kHz`, `5 ms`, `1.5 s`, `40%`. Three significant digits at most, no zeros at the end, so a value at rest is short.
- Focus: the arrangement and the note editor each show a 1 px lavender ring inside their edge, only when the focus came from the keyboard. A knob shows it as a 2 px lavender ring around its face, because 1 px on a 35 px circle was too weak to find, and a segmented control as its 1 px border, under the same rule. Tab goes from the project menu to the arrangement, then the note editor and its close icon, or the close icon of the track panel and its controls from left to right, which begins with the picker of the first card and ends with the control that adds an effect, then the transport: play, stop, record, the seek strip, the tempo and the click. Since step 1b of the third milestone the transport comes right after the project menu, see "Settled when the window was built".
- Cursor: a left-right resize cursor over the edges of a clip and over the end of a note, 6 px wide or a quarter of a narrow shape. Nothing else changes on hover.
- Scroll pans, pinch or cmd-scroll zooms in time about the pointer. A click on a ruler seeks to the nearest step of the snap setting (a sixteenth before step 9a of the third milestone). The rest is under "Using the app".

## Using the app

`Sound Tools.app`, or `cargo run -p runtime -- <project-folder>`, opens the window. The app opens the last project, or the macOS folder panel when there is none; when a project does not open, a small window says why and has **Choose folder…**. Everything snaps to the snap setting in the corner above the track headers, a sixteenth when the window opens; cmd held during a drag bypasses it. Every drag and every key below that changes the piece is one undo step, and escape during a drag puts it back. The keys follow macOS: cmd-c, cmd-x, cmd-v, cmd-d, cmd-a, delete, shift-click and cmd-click, enter to rename.

| Where | Mouse or key | What it does |
| --- | --- | --- |
| Anywhere | space | Play or pause |
| Anywhere | r | Record from the playhead on every armed audio track and on the selected instrument track; with nothing armed and an audio track selected, arm it and record. Again to end the take |
| Anywhere | cmd-z, shift-cmd-z | Undo, redo. Both wait while a drag is going on |
| Anywhere | tab, shift-tab | Move the focus: project menu, transport, arrangement, the panel below |
| Project menu | Add track, Instrument track | A new track with a synth |
| Project menu | Add track, Audio track | A new empty audio track |
| Project menu | Fit tempo to take | Fit the project tempo to the take of the selected clip. One undo step |
| Project menu | Open project… | Pick another project folder, or make a new empty one, in the macOS folder panel. The app quits as with cmd-q and opens again on it. A folder with other files and no `project.json` is refused |
| Project menu | Open terminal in project folder | The macOS Terminal in the folder, to start a coding agent there |
| Project menu | Install command line tool | Link `sound-tools` to this app in `/usr/local/bin`, or in `~/.local/bin` when that needs an administrator. A dialog says where, and how to add `~/.local/bin` to the `PATH`. Agents run `sound-tools . --inspect` |
| Ruler | click | Move the playhead there, on the grid |
| Ruler | double click, or `t` in the arrangement | Add a tempo change there, or at the playhead. It keeps the tempo that played there until it is edited |
| Ruler | click a tempo mark (`96 bpm`) | Select it and move the playhead onto it: the tempo of the transport is then its tempo, and a drag there edits it |
| Arrangement | delete or backspace, with a tempo mark selected | Remove that tempo change. Escape lets go of it |
| Arrangement | Snap, in the corner above the track headers | Off, Bar, Beat, 1/8, 1/16 or 1/32 for every drag, new clip, new note and arrow key. Not saved |
| Arrangement or note editor | cmd while dragging | Bypass the snap. A cmd press on a clip that moves is a drag of the selection with it; one that does not move is a cmd-click |
| Transport | drag the tempo up or down | Change the tempo at the playhead. Whole bpm from where it began, half a bpm per pixel. With shift tenths, at a tenth of the speed. Escape during the drag puts it back |
| Transport | up or right, down or left on the focused tempo | One bpm. With shift a tenth |
| Transport | drag the steadiness up or down | How steady the fitted tempo is, 0 as played to 100 one tempo. One percent per pixel, with shift tenths. Escape during the drag puts it back. It shows only when the project has a fit |
| Transport | up or right, down or left on the focused steadiness | Five percent. With shift one |
| Transport | the metronome button | The click on or off. It is not an undo step and changes no file |
| Transport | the record button | The same as `r` |
| Arrangement | the circle in the header of an audio track | Arm or disarm it: an armed track shows the level of its input and records on `r`. Not saved and no undo step |
| Track panel | the input select of an audio track, `In 1` | Which channels of the default input of macOS it records: one alone, or a pair for a stereo take. One undo step |
| A MIDI keyboard | any key, and the sustain pedal | Plays the instrument of the selected track, whether the project plays or not |
| Arrangement or note editor | scroll, cmd-scroll or pinch | Pan, zoom in time |
| Arrangement | double click on empty space of an instrument track | Add a clip of one bar. On an audio track it does nothing: audio clips come from files |
| Arrangement | click on a clip | Select it |
| Arrangement | shift-click or cmd-click on a clip | Add it to the selection, or take it out |
| Arrangement | drag on empty track space | Select the clips the rectangle touches. With shift or cmd add them |
| Arrangement | cmd-a | Select every clip |
| Arrangement | cmd-c, cmd-x | Copy, cut the selected clips. The clipboard is in the app only |
| Arrangement | cmd-v | Paste at the playhead. The top row goes on the track of the first selected clip, else on the selected track, else on the first track. Rows below the last track land on the last track |
| Arrangement | cmd-d | A copy of the selected clips right after them |
| Arrangement | drag a clip | Move it, and every other selected clip, in time and to another track |
| Arrangement | drag the left or right edge of a clip | Resize it. The left edge stops at the first note. On an audio clip it trims the file and keeps the sound where it is in time |
| Arrangement | drag a fade handle or the gain handle of an audio clip | Fade it in or out, or change its gain. Shift makes the gain finer |
| Arrangement | alt-up, alt-down | The gain of the selected audio clips by 1 dB |
| Arrangement | drop audio files from the Finder | Copy them into `assets/audio/` and add them as clips one after another on that audio track, or on a new audio track under the last one. One undo step |
| Arrangement | delete or backspace | Delete the selected clips |
| Arrangement | left, right, up, down | Move the selected clips by a step of the snap (a sixteenth when it is off), or to the track above or below |
| Arrangement | double click on a clip, or enter | Open the note editor for it. It takes the place of the track panel. For an audio clip, open the panel of its track, with the clip in its Clip card |
| Arrangement | click on a track header | Select the track and open its track panel. It takes the place of the note editor |
| Arrangement | up, down, with a track and no clip selected | Select the track above or below. The open track panel follows |
| Arrangement | enter, with a track and no clip selected, or a double click on a track header | Edit the name of the track. Enter or a click elsewhere keeps it, escape does not |
| Arrangement | cmd-down, with a track and no clip selected | Open the track panel |
| Arrangement or track panel | escape | Close the panel below |
| Track panel | the close icon | Close the panel |
| Track panel | drag a knob up or down | Change the value. The sound follows. Escape during the drag puts it back |
| Track panel | the volume, under the track name | The level of the track in decibels: drag the thumb or the meter, double click for 0 dB, arrows by 0.5 dB and with shift 0.1 dB |
| Track panel | Pan | Where the track sits between the two channels |
| Track panel | M | Silence the track, and click again to bring it back |
| Track panel | S | Hear this track and the other soloed ones alone, and click again to hear all |
| Track panel | the power icon of an effect card | Bypass the effect: the sound goes past it untouched. One undo step |
| Arrangement | click the master row under the tracks, or tab to it and enter | Open the master panel: the master volume and the limiter |
| Master panel | Gain, Ceiling, Release, and Lookahead behind expand | The limiter. The output never goes over the ceiling. Drag the handle of the ceiling line, or the knob |
| Master panel | the power icon of the Limiter | Switch the limiter off, and the output may clip |
| Track panel | drag a handle of a display | The value it moves, as its knob does. With shift ten times finer, double click resets. Escape during the drag puts it back |
| Track panel | click a numbered handle of the EQ, or 1 to 4 on a focused control of its card | Select that band: its knobs and shape show on the card. No undo step |
| Track panel | the expand icon of a card | Show the controls the card hides, such as the envelope knobs of the synth |
| Track panel | double click on a knob, or backspace on the focused knob | Set its default |
| Track panel | shift while dragging a knob | Ten times finer, from where the value is |
| Track panel | up or right, down or left on a focused knob | One step, a fiftieth of the travel. With shift a five-hundredth |
| Track panel | click on a waveform, or left and right on the focused control | Switch the waveform |
| Track panel | click the name on a card | Pick another instrument or effect for that card. One undo step |
| Track panel | Add effect, at the end of the rack | Put an effect at the end of the chain. One undo step |
| Track panel | the close icon in the header of an effect card | Take that effect off the track. One undo step, and undo brings it back as it sounded |
| Track panel | drag the header of an effect card onto another card | Move the effect to that place in the chain. On the instrument it goes first, on `Add effect` last. One undo step. It keeps its bypass. Escape during the drag lets go |
| Track panel | cmd-left, cmd-right, in an effect card | Move that effect one place. The instrument stays first |
| Track panel | drop an audio file from the Finder on the display of a Sampler, or `Choose file` on it | Copy the file into the project and play it across the keyboard. One undo step, "Load sample" |
| Track panel | up or down on the focused Root knob of a Sampler | One semitone |
| Drum pad card | press a pad | Select it and play it. No undo step |
| Drum pad card | tab to the grid, then the arrows, enter | The grid is one stop: the arrows move the selection, enter plays the selected pad |
| Drum pad card | drop a file from the Finder on a pad | Copy it into `assets/audio/` and make that pad play it. One undo step. `Choose file…` in the Sound list does the same with the file panel |
| Track panel | Open window, on the card of a plugin | The plugin's own window, above this one, where it was the last time. The same control closes it |
| Note editor | double click on empty space inside the clip | Add a note of one step of the snap. Keep the second press down and drag to draw its length. It sounds |
| Note editor | click on a note | Select it. It sounds |
| Note editor | shift-click or cmd-click on a note | Add it to the selection, or take it out |
| Note editor | drag on empty space | Select the notes the rectangle touches. With shift or cmd add them |
| Note editor | cmd-a | Select every note of the clip |
| Note editor | drag a note | Move it, and every other selected note, in time and pitch. A new pitch sounds |
| Note editor | drag the end of a note | Change its length |
| Note editor | delete or backspace | Delete the selected notes. Undo brings them back selected |
| Note editor | left, right | Move the selected notes by a step of the snap |
| Note editor | up, down, with shift | Move them by a semitone, by an octave |
| Note editor | cmd-c, cmd-x | Copy, cut the selected notes. The clipboard is the one of the timeline: a copy of notes replaces copied clips |
| Note editor | cmd-v | Paste at the playhead when it is in the clip, else right after the selected notes, else at the start of the clip. A note that would start past the clip end is left out |
| Note editor | cmd-d | A copy of the selected notes right after them |
| Note editor | drag a bar of the velocity lane up or down | Change the velocity of its note, and of every selected note with it |
| Note editor | drag across the velocity lane from off a bar | Draw: every bar it passes gets the height of the pointer there. One undo step |
| Note editor | alt-up, alt-down | The velocity of the selected notes up or down by 10 |
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
