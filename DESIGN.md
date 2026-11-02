# Design

The design principles of the Sound Tools UI, and the rules of how it is used. What each click and key does is in the code of each view. Colours, sizes and components live in code: the theme and components in `crates/ui` (see [crates/ui/README.md](crates/ui/README.md)), and the gallery in `crates/gallery` shows every component in every state. Mockups drawn for review are in `docs/mockups/`.

## Principles

### The quiet rule

Every element must earn its keep. Lots of air, colour only where it means something, calm type. Audio tools are usually crowded; we do not copy that.

- A control that does nothing yet is not shown. It is noise.
- No legends, no zoom controls, no scrollbars. The timeline has no grid, only lines that mean something: a faint line between tracks, and every other section of the song a shade lighter. The note editor is the one place with a grid, because notes are placed by it.
- A track's head shows its name and what plays it. Of its toggles only those that are on show, and all of them while the pointer is over it or its panel is open.
- No toolbars and no tools: what the pointer is on decides what a drag does.
- Chrome is one quiet menu on the project name, and the live input beside it.
- Cards have no meta chips, rack ears, screws or gradients. Secondary parameters sit behind the expand icon of a card.
- Text in the window is for the composer, a musician, not for a programmer: no names of languages, programs, formats or files (TypeScript, Bun, JSON, `extensions/`). Something that cannot run says why in plain words (`This project does not load plugins.`); what to edit is for the agent docs.
- Notices are quiet lines in a corner. Nothing blocks.
- Accessible: visible focus rings for keyboard focus, labelled controls, full keyboard reach. This is a product requirement.

### One calm instrument

- Every device shows what it does to the sound. A card is a display of the sound on the left and a few knobs at its right. The handles on a display drag.
- A display is never the only way to reach a value: every value a handle moves also has a knob, visible or behind expand. That knob is the path for the keys.
- A value is one bright line on dark: the arc of a knob, the thumb on a meter, a curve and its handles. Values are never coloured.
- One control per value. The tempo is a drag number in the transport; a mark in the ruler or the tempo lane selects and seeks.
- Every drag control shares one gesture: it moves from where the value is (a press never jumps), shift is ten times finer, double click or backspace sets the default, escape during a drag puts it back.
- A value an automation lane of the track moves is not the composer's to drag: its control shows what the lane plays at the playhead and stays put, and its keys and reset do nothing. It stays a tab stop, and a small dot and a tooltip say why. Taking the lane out gives the control back the record.
- Two-finger scroll never changes a value. A control that took the gesture would change a sound while the composer scrolls past.
- Interface state that is about how the composer works (zoom, snap, the loop, expanded cards, shown lanes, open returns, armed tracks) is not saved in the project. A setting in a project file would be one more thing an agent could change under the composer. What belongs to the machine is kept in the app's support folder, outside every project: whether the agent sidebar is open, the agent's approval mode and model, and its threads.
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
- One grid for devices: every card is the same height, with two rows of fixed cells of a knob, a label line and a value line. Cards grow wider, never taller, so a rack has a straight bottom. Empty cells are fine.
- Hit targets are larger than what is drawn, for a trackpad.

## Using the app

The DAW template follows these rules; its own gestures and keys are its code, in `templates/daw/extensions`.

### Gestures

- Every drag and every key that changes the piece is one point of history. Undo and going back wait while a drag or a take is going on, and say so.
- History is exploring, never commits or branches: every state is kept, small steps fold away on their line, and an idea tried is a version. Going back is one click and makes nothing new; the next change does.
- There is no save. Every finished edit is already in the folder.
- Everything snaps to the grid of the timeline. Cmd during a drag ignores it. Arrow keys move things in time by one snap step.
- Two fingers pan. Cmd-scroll or pinch zooms in time, about the pointer.
- Click selects, shift- or cmd-click adds or takes out, a drag on empty space selects with a rectangle, cmd-a selects all. Cmd-c, cmd-x, cmd-v (at the playhead) and cmd-d (right after) copy, cut, paste and duplicate; delete removes.
- Double click on empty space makes what lives there: a clip, a note. On a control it sets the default.
- Alt-drag copies.
- Adding something never changes the sound: a new automation lane holds the value its number plays now.
- A drag shows what it will do before the drop: a lavender ring where it lands.
- Audio comes from files only. A dropped file is copied into the project.
- Arming, the metronome and the computer keys are not saved and are no undo step.

### Keys

- The keys follow macOS. On Windows and Linux ctrl takes the place of cmd.
- A key plays or stops only where a tool takes it, and not while a text field has the keys. Keys with cmd and tab always stay the window's, so the focus can always move on.
- A group of options is one tab stop, such as a segmented control or the drum pads. The arrows move inside it.
- Escape lets go, closes what is open, or gives the focus back.
- The computer keys (cmd-K) play what is selected like a MIDI keyboard, and their letters then do nothing else. Both play it whether the project plays or not.

### The agent

- One message at a time. Everything the agent changes for one message is one point of history.
- The sidebar keeps its last thread per project, and the agent goes on where it left off.
- While the sidebar is closed and the agent works or asks, its icon carries a lavender dot.
