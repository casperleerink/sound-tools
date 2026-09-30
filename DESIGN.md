# Design

The design principles of the Sound Tools UI, and every mouse action and key of the app. Colours, sizes and components live in code: the theme and components in `crates/ui` (see [crates/ui/README.md](crates/ui/README.md)), and the gallery in `crates/gallery` shows every component in every state. Mockups drawn for review are in `docs/mockups/`.

## Principles

### The quiet rule

Every element must earn its keep. Lots of air, colour only where it means something, calm type. Audio tools are usually crowded; we do not copy that.

- A control that does nothing yet is not shown. It is noise.
- No legends, no zoom controls, no scrollbars, no grid in the arrangement. The note editor is the one place with a grid, because notes are placed by it.
- No toolbars and no tools: what the pointer is on decides what a drag does.
- Chrome is one quiet menu on the project name. The transport is the one floating shape, in the title row, and floats over nothing.
- Cards have no meta chips, rack ears, screws or gradients. Secondary parameters sit behind the expand icon of a card.
- Something that cannot run says why in plain words (`This project does not load plugins.`), never as a file edit. File edits are for the agent docs.
- Notices are quiet lines in a corner. Nothing blocks.
- Accessible: visible focus rings for keyboard focus, labelled controls, full keyboard reach. This is a product requirement.

### One calm instrument

- Every device shows what it does to the sound. A card is a display of the sound on the left and a few knobs at its right. The handles on a display drag.
- A display is never the only way to reach a value: every value a handle moves also has a knob, visible or behind expand. That knob is the path for the keys.
- A value is one bright line on dark: the arc of a knob, the thumb on a meter, a curve and its handles. Values are never coloured.
- One control per value. The tempo has one drag number, in the transport; a tempo mark in the ruler selects and seeks.
- Every drag control shares one gesture: it moves from where the value is (a press never jumps), shift is ten times finer, double click or backspace sets the default, escape during a drag puts it back.
- Two-finger scroll never changes a value. A control that took the gesture would change a sound while the composer scrolls past.
- Interface state that is about how the composer works (zoom, snap, expanded cards, armed tracks) is not saved. A setting in a file would be one more thing an agent could change under the composer.
- Our own look: plain parameter names, our own drawings on the displays. We learn from Ableton but copy no graphic, name or text of it.

### Colour

Dark only. The greys run from the darkest background to the text colour, and one alpha scale (white at low opacity) makes fills, hovers and borders. The palette is our own: a public theme many apps wear is not an identity.

Each colour has one meaning:

- Green: sound is moving. Play, a meter below -6 dBFS, a level inside a display, a sounding drum pad, where the sound plays in a waveform display.
- Yellow: a meter from -6 to 0 dBFS, and solo.
- Peach: warning, files not live, and mute.
- Red: record, the clip light of a meter, errors, an armed track, a take while it records.
- Lavender: keyboard focus, the agent, and the ring where a drag or a dropped file lands.
- Track colours: marks only. Dots, notes, velocity bars, the line of a bend, mod wheel or pressure lane, the waveform of an audio clip. No control uses a track colour, because track colours include green, yellow, peach and red.

A meter is the one place colour fills an area, because level is a signal. Otherwise only a toggle that is on is tinted, with its colour at low opacity.

Text must reach 4.5 : 1 contrast on its surface, icons 3 : 1. A test in `crates/ui/src/theme.rs` checks the text tokens.

### Type and sizes

- InterDisplay with tabular numbers for every number, so readouts and the transport do not jitter while playing. Two sizes in the rack: 14 medium for titles and names, 12 for labels and values.
- The target is a 13 to 14 inch MacBook with a trackpad: a 1470 x 920 point window. The snapshot tests render at that size.
- An 8 pt layout grid, 4 pt inside controls.
- One grid for devices: every card is the same height, with two rows of fixed cells of a knob, a label line and a value line. Cards grow wider, never taller, so the rack has a straight bottom and fits the panel below the arrangement. Empty cells are fine.
- Hit targets are larger than what is drawn, for a trackpad.

## Using the app

`Sound Tools.app`, or `cargo run -p runtime -- <project-folder>`, opens the window. It opens the last project, or the folder panel when there is none. When a project does not open, a small window says why and has **Choose folder…**.

Everything snaps to the snap setting in the corner above the track headers (1/16 when the window opens). Hold cmd during a drag to ignore it. Every drag and every key that changes the piece is one undo step, and escape during a drag puts it back. The keys follow macOS.

| Where | Mouse or key | What it does |
| --- | --- | --- |
| Anywhere | space | Play or pause |
| Anywhere | r | Record from the playhead on every armed audio track and on the selected instrument track. With nothing armed and an audio track selected, arm it and record. Again to end the take. Stop, pause or a seek also end it |
| Anywhere | cmd-z, shift-cmd-z | Undo, redo. Both wait while a drag is going on |
| Anywhere | tab, shift-tab | Move the focus: project menu, transport, arrangement, the panel below |
| Anywhere | cmd-q | Quit. There is no save: every finished edit is already in the folder |
| A MIDI keyboard | any key, the sustain pedal, the bend and mod wheels, key pressure | Play the instrument of the selected track, whether the project plays or not. The synth, the Wavetable and the Sampler bend two semitones and add a vibrato with the mod wheel; the Wavetable can also route the mod wheel and the key pressure in its matrix. A plugin gets them as MIDI (CLAP) or on the parameters it maps them to (VST 3). A take records the wheels too |
| Project menu | Fit tempo to take | Fit the tempo map to the take of the selected clip, so bar lines land on the beats as played |
| Project menu | Undo, Redo | Named after the step they undo or redo |
| Project menu | Output device | Shows the device the app plays on |
| Project menu | Open project… | Pick another project folder, or make a new one. The app reopens on it. A folder with other files and no `project.json` is refused |
| Project menu | Reveal project folder | Show the folder in the Finder |
| Project menu | Open terminal in project folder | A terminal in the folder, to start a coding agent there |
| Project menu | Install command line tool | Link `sound-tools` into `/usr/local/bin`, or `~/.local/bin` when that needs an administrator. A dialog says where |
| Transport | play, stop, record buttons | Play or pause, stop, the same as `r` |
| Transport | click or drag on the seek strip; left, right when focused | Move the playhead. The keys move by a bar. Only when the project has an end |
| Transport | drag the tempo up or down | The tempo at the playhead. Half a bpm per pixel in whole bpm, with shift in tenths |
| Transport | arrows on the focused tempo | One bpm, with shift a tenth |
| Transport | drag the steadiness up or down | Only with a fit: 0 is as played, 100 is one steady tempo. One percent per pixel, with shift tenths |
| Transport | arrows on the focused steadiness | Five percent, with shift one |
| Transport | the metronome button | The click on or off. Not an undo step, changes no file, never in a render |
| Ruler | click | Move the playhead there, on the snap |
| Ruler | double click, or `t` in the arrangement | Add a tempo change there, or at the playhead. It keeps the tempo that played there |
| Ruler | click a tempo mark | Select it and move the playhead onto it. The transport tempo then edits it |
| Arrangement | delete or backspace, with a tempo mark selected | Remove that tempo change. Escape lets go of it |
| Arrangement | Snap select | Off, Bar, Beat, 1/8, 1/16 or 1/32. Not saved |
| Arrangement | Add track, under the last track header | A new track with a synth |
| Arrangement | the chevron next to Add track: Instrument track, Audio track | A new track with a synth, or a new empty audio track |
| Arrangement, note editor | scroll; cmd-scroll or pinch | Pan; zoom in time about the pointer |
| Arrangement | double click on empty space of an instrument track | Add a clip of one bar. Audio clips come from files only |
| Arrangement | click; shift-click or cmd-click on a clip | Select it; add it to the selection or take it out |
| Arrangement | drag on empty track space | Select the clips the rectangle touches. With shift or cmd, add them |
| Arrangement | cmd-a | Select every clip |
| Arrangement | cmd-c, cmd-x | Copy, cut the selected clips. The clipboard is in the app only |
| Arrangement | cmd-v | Paste at the playhead. The top row goes on the track of the first selected clip, else the selected track, else the first track |
| Arrangement | cmd-d | A copy of the selected clips right after them |
| Arrangement | drag a clip | Move it, with every selected clip, in time and to another track |
| Arrangement | drag the left or right edge of a clip | Resize it. The left edge stops at the first note. On an audio clip it trims the file and keeps the sound in place |
| Arrangement | drag a fade handle or the gain handle of an audio clip | Fade in or out, or change its gain. The value shows while dragging |
| Arrangement | alt-up, alt-down | The gain of the selected audio clips by 1 dB |
| Arrangement | drop audio files from the Finder | Copy them into `assets/audio/` and add them one after another on that audio track, or under the last track on a new audio track |
| Arrangement | delete or backspace | Delete the selected clips |
| Arrangement | arrows | Move the selected clips by a snap step (1/16 when off), or to the track above or below |
| Arrangement | double click on a clip, enter or cmd-down | Open it: the note editor for a note clip, the track panel with its Clip card for an audio clip |
| Arrangement | click a track header | Select the track and open its track panel |
| Arrangement | up, down, with a track and no clip selected | Select the track above or below. The open panel follows |
| Arrangement | drag a track header up or down | Move the track there, with its clips and devices. A lavender ring shows where it lands. The master stays last |
| Arrangement | alt-up, alt-down, with a track and no clip selected | Move the track one place up or down |
| Arrangement | enter with a track selected, or double click a track header | Rename the track. Enter or a click elsewhere keeps it, escape does not |
| Arrangement | cmd-down, with a track and no clip selected | Open the track panel |
| Arrangement | the circle in an audio track header | Arm or disarm it. An armed track shows its input level and records on `r`. Not saved, no undo step |
| Arrangement | click the master row, or tab to it and enter | Open the master panel: the master volume and the limiter |
| Arrangement, track panel | escape | Close the panel below |
| Track panel | the close icon | Close the panel |
| Track panel | the volume under the track name | Drag the thumb or the meter; double click for 0 dB; arrows 0.5 dB, with shift 0.1 dB |
| Track panel | Pan, M, S | Pan the track; mute it; solo it (with other soloed tracks) |
| Track panel | the input select of an audio track | Which channels of the default input it records: one, or a pair for stereo |
| Track panel | drag a knob up or down | Change the value. With shift ten times finer |
| Track panel | arrows on a focused knob | A fiftieth of the travel, with shift a five-hundredth |
| Track panel | double click a knob, or backspace on it | Set its default |
| Track panel | drag a handle of a display | Change what it moves, as its knob does. Shift is finer, double click resets |
| Track panel | click a segment, or left and right on it | Pick an option, such as the synth waveform or the filter type |
| Track panel | the expand icon of a card | Show the controls the card hides. Not saved |
| Track panel | the power icon of an effect card | Bypass the effect. On the limiter of the master, the output may then clip |
| Track panel | click the name on a card | Pick another instrument or effect for that slot |
| Track panel | Add effect, at the end of the rack | Put an effect at the end of the chain |
| Track panel | the close icon of an effect card | Remove it. Undo brings it back as it sounded |
| Track panel | drag the header of an effect card onto another card | Move the effect there. On the instrument it goes first, on `Add effect` last. It keeps its bypass |
| Track panel | cmd-left, cmd-right in an effect card | Move that effect one place |
| Track panel | Open window, on a plugin card | The plugin's own window, above this one, where it was last time. Again to close it |
| EQ card | click a numbered handle, or 1 to 4 on a focused control | Select that band. No undo step |
| Sampler card | drop an audio file on the display, or `Choose file` | Copy it into the project and play it across the keyboard |
| Sampler card | up, down on the focused Root knob | One semitone |
| Drum pad card | press a pad | Select it and play it. No undo step |
| Drum pad card | tab to the grid, arrows, enter | The grid is one stop. The arrows move the selection, enter plays the pad |
| Drum pad card | drop a file on a pad, or `Choose file…` in the Sound list | Copy it into `assets/audio/` and make that pad play it |
| Note editor | double click on empty space in the clip | Add a note of one snap step. Keep the second press down and drag to draw its length. It sounds |
| Note editor | click; shift-click or cmd-click on a note | Select it, it sounds; add it to the selection or take it out |
| Note editor | drag on empty space | Select the notes the rectangle touches. With shift or cmd, add them |
| Note editor | cmd-a | Select every note of the clip |
| Note editor | drag a note | Move it, with every selected note, in time and pitch. A new pitch sounds |
| Note editor | drag the end of a note | Change its length |
| Note editor | delete or backspace | Delete the selected notes. Undo brings them back selected |
| Note editor | left, right | Move the selected notes by a snap step (1/16 when off) |
| Note editor | up, down; with shift | Move them by a semitone; by an octave |
| Note editor | cmd-c, cmd-x | Copy, cut the selected notes. Copied notes replace copied clips in the clipboard |
| Note editor | cmd-v | Paste at the playhead when it is in the clip, else after the selected notes, else at the clip start. Notes past the clip end are left out |
| Note editor | cmd-d | A copy of the selected notes right after them |
| Note editor | drag a velocity bar up or down | Change its velocity, and that of every selected note |
| Note editor | drag across the velocity lane from off a bar | Draw: every bar passed gets the height of the pointer there |
| Note editor | alt-up, alt-down | The velocity of the selected notes by 10 |
| Note editor | the select left of the lane | Show the velocities, or the bend, mod wheel or pressure of the clip, one at a time. Not saved |
| Note editor | drag across the bend, mod wheel or pressure lane | Draw its line: the points under the drag give way to the height of the pointer, one per snap step, or every few pixels with snap off or cmd. The bend has its middle line at 0 |
| Note editor | alt-drag across that lane | Erase the points it covers |
| Note editor | double click in that lane | Clear it: the lane plays at rest |
| Note editor | click a key of the strip | Hear that pitch |
| Note editor | escape or the close icon | Close the editor |
