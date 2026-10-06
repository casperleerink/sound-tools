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
- A value an automation lane of the track moves is not the composer's to drag: its knob, volume or handle shows what the lane plays at the playhead and stays put, and its arrow keys and reset do nothing. It stays a tab stop, so the keys still reach it, and a drag that is open when its lane arrives ends. A knob and a volume carry a 4 pt dot at their top right, in the grey of a label and not a track colour, because it is a control, and a tooltip says `Follows the automation of the track`. Taking the lane out gives the control back the record.
- Two-finger scroll never changes a value. A control that took the gesture would change a sound while the composer scrolls past.
- Interface state that is about how the composer works (zoom, snap, expanded cards, shown lanes, armed tracks) is not saved in the project. A setting in a project file would be one more thing an agent could change under the composer. What belongs to the machine is kept in the app's support folder, outside every project: whether the agent sidebar is open, the agent's approval mode and model, and its threads.
- Our own look: plain parameter names, our own drawings on the displays. We learn from Ableton but copy no graphic, name or text of it.
- Every built-in device has its own icon in the pickers: a line drawing of what it does to the sound, on the 24 pt grid of the other icons. A plugin shows a plug. The built-in instruments are one group; the built-in effects are grouped by what they do (Tone, Dynamics, Space, Mix), with the plugins last.

### Colour

Dark only. The greys run from the darkest background to the text colour, and one alpha scale (white at low opacity) makes fills, hovers and borders. The palette is our own: a public theme many apps wear is not an identity.

Each colour has one meaning:

- Green: sound is moving. Play, a meter below -6 dBFS, a level inside a display, a sounding drum pad, where the sound plays in a waveform display.
- Yellow: a meter from -6 to 0 dBFS, and solo.
- Peach: warning, files not live, and mute.
- Red: record, the clip light of a meter, errors, an armed track, a take while it records.
- Lavender: keyboard focus, the agent, and the ring where a drag or a dropped file lands.
- Track colours: marks only. Dots, notes, velocity bars, the line of a bend, mod wheel, pressure or automation lane, the waveform of an audio clip. No control uses a track colour, because track colours include green, yellow, peach and red.

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
| Anywhere | tab, shift-tab | Move the focus: the sidebar icon, project menu, transport, the agent sidebar, arrangement, the panel below |
| Anywhere | cmd-L | Open the agent sidebar with the focus in its composer. In the composer, close it again |
| Anywhere | cmd-q | Quit. There is no save: every finished edit is already in the folder |
| A MIDI keyboard | any key, the sustain pedal, the bend and mod wheels, key pressure | Play the instrument of the selected track, whether the project plays or not. The synth, the Wavetable and the Sampler bend two semitones and add a vibrato with the mod wheel; the Wavetable can also route the mod wheel and the key pressure in its matrix. A plugin gets them as MIDI (CLAP) or on the parameters it maps them to (VST 3). A take records the wheels too |
| Title row | the sidebar icon, right of the traffic lights | Open or close the agent sidebar. It stays as it is at the next start. While it is closed and the agent works or asks, the icon carries a lavender dot |
| Agent sidebar | **Set up**, at the first use | Download the pinned Claude Code (215 MB), then sign in. **Cancel** stops; **Try again** resumes |
| Agent sidebar | Sign in with your Claude plan, Use an Anthropic Console account (API) | Run Claude Code's own sign-in in the browser. **Open the page again** starts it again, **Cancel** stops it |
| Agent sidebar | the two pills in the composer | The model pill shows the provider's logo and the model; its menu lists the provider's models, the email and plan, and **Sign out**. The access pill shows how much the agent may do without asking (Ask always, Ask for commands, Full access; plain reads such as `ls` never ask). Both are kept for the machine, are the same in every window, and apply at once, also to a turn that runs |
| Agent sidebar | up, down in the composer | Up in an empty composer, or on its first row, recalls the earlier messages of the thread, newest first. Down goes forward again, and past the newest to empty. A draft stays |
| Agent sidebar | enter in the composer, or the send button | Send the message. The agent works in the project folder; everything it changes for one message is one undo step named after the message. One message at a time |
| Agent sidebar | shift-enter in the composer | Add a line |
| Agent sidebar | the stop button, or cmd-period | Stop the agent's turn |
| Agent sidebar | escape in the composer | Give the focus back to where it was |
| Agent sidebar | **+** | A new thread. The agent of the old one stops, and the old one stays saved. The sidebar opens on the project's last thread, and the agent goes on where it left off |
| Agent sidebar | Allow, Allow for this thread, Deny | Answer the agent when it asks before an edit or a command, as the approval mode says |
| Agent sidebar | Worked for … | Show the steps of that turn. A failed step says so in red; a denied one says what it asked to do, and denied |
| Agent sidebar | N files are not live, under an answer | Show the problems that turn left, as `path: message` lines |
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
| Arrangement | cmd-c, cmd-x | Copy, cut the selected clips with the automation under them. A cut leaves a straight line where they were. The clipboard is in the app only |
| Arrangement | cmd-v | Paste at the playhead, with the automation, over the automation there. The top row goes on the track of the first selected clip, else the selected track, else the first track |
| Arrangement | cmd-d | A copy of the selected clips right after them, with their automation |
| Arrangement | drag a clip | Move it, with every selected clip, in time and to another track. The automation under it goes along and replaces what is where it lands; just outside, nothing changes. A lane goes along only when it has a point inside the clip and does not hold one value all along. To another track only the volume and the pan go: device lanes stay with their track |
| Arrangement | alt-drag a clip | Move it without its automation. Alt can be pressed or let go during the drag |
| Arrangement | while a clip drag takes automation along | In each lane it takes, the line it takes follows over a light band, and the line it replaces fades. `Automation moves · alt to leave it` shows in the top left of the clip. With the lanes folded away, the clip carries a small mark instead. What shows is what drops |
| Arrangement | **Automation**, the second line of a track header, or `a` with a track and no clip selected | Show its automation lanes under it, or fold them away. It counts the lanes (`Automation · 2`) and is brighter when the track has any. Not saved, no undo step |
| Arrangement | Add lane, under the lanes of a track | Pick a number of the track (volume, pan) or of one of its devices that has no lane yet, every parameter of a plugin that takes one included; type to search. The lane holds the value it plays now, so nothing sounds different yet. A plugin parameter that is not on its card goes on it in the same undo step. Tab reaches it. Gone when every number has a lane |
| Arrangement | an automation lane | Each point is a dot on the line, in the track colour. Over a dot the cursor is a hand and the dot grows; elsewhere in the lane it is a crosshair: a press there adds a point |
| Arrangement | click in an automation lane, off a dot | Add a point there, on the grid (cmd: off the grid), at the height of the pointer, on the travel of the knob. It is selected, and a drag before the button comes up moves it. One undo step |
| Arrangement | click a dot / drag a dot | Select the point (a ring shows it) / move it, on the grid unless cmd is held. A point stays between its neighbours. Above or below the lane is the end of the range |
| Arrangement | shift while dragging a dot | Only up and down or only sideways: the way the pointer went furthest. Pressed or let go during the drag, it takes effect at once |
| Arrangement | delete, with a point selected | Delete it. Without points the lane goes, and the knob gets the value of its record back. Escape lets go of the point |
| Arrangement | alt-drag across an automation lane | Erase the points it covers. Without points the lane goes |
| Arrangement | drag the left or right edge of a clip | Resize it. The left edge stops at the first note. On an audio clip it trims the file and keeps the sound in place |
| Arrangement | drag a fade handle or the gain handle of an audio clip | Fade in or out, or change its gain. The value shows while dragging |
| Arrangement | alt-up, alt-down | The gain of the selected audio clips by 1 dB |
| Arrangement | drop audio files from the Finder | Copy them into `assets/audio/` and add them one after another on that audio track, or under the last track on a new audio track |
| Arrangement | delete or backspace | Delete the selected clips |
| Arrangement | arrows | Move the selected clips by a snap step (1/16 when off), or to the track above or below, with their automation as a drag does |
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
| Track panel | a knob, volume or handle with a dot, or one that does not move | An automation lane of the track moves it: it shows what the lane plays and does not drag. Edit or delete the lane to change it |
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
| Plugin card | Parameters | Search the plugin's parameters: a pick puts one on the card at the value it has, a checked one comes off, unless a lane moves it. 64 at most |
| Plugin card | a parameter on the card | Two steps are a toggle, named steps a dropdown, the rest a knob, with the plugin's own text for the value; `Not found` or `Out of range` says what is wrong |
| EQ card | click a numbered handle, or 1 to 4 on a focused control | Select that band. No undo step |
| Sampler card | drop an audio file on the display, or `Choose file` | Copy it into the project and play it across the keyboard |
| Sampler card | up, down on the focused Root knob | One semitone |
| Drum pad card | press a pad | Select it and play it. No undo step |
| Drum pad card | tab to the grid, arrows, enter | The grid is one stop. The arrows move the selection, enter plays the pad |
| Drum pad card | drop a file on a pad, or `Choose file…` in the Sound list | Copy it into `assets/audio/` and make that pad play it |
| Wavetable card | drag up or down anywhere on a wavetable | Move the position of that oscillator through its frames, as its Position knob does. The bright line is the frame that plays. Shift is finer, double click resets |
| Wavetable card | the table at the top of a wavetable | Pick the table of that oscillator, grouped by kind |
| Wavetable card | Osc, Voice, Filter, Env, LFO, Matrix in the header of the expanded card | Which page of sections it shows. A dot marks a page that differs from the default patch. Not saved, no undo step |
| Wavetable card | Amp, Env 2, Env 3; LFO 1, LFO 2 | Which envelope or LFO its section shows, with its knobs. Not saved, no undo step |
| Wavetable card | Add route, under the routes of the matrix | A new route from LFO 1 to the position of Osc 1, at no amount. Up to 16 |
| Wavetable card | the source, the destination and the line of a route | Pick where it comes from and where it goes. Drag the line sideways for the amount, from the middle either way; shift is finer, double click is none. Two fingers scroll a long list |
| Wavetable card | the close icon of a route | Remove it |
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
