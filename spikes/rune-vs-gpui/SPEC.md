# The screen both apps draw

One window, 1440x900 pt, dark. Same layout, same data (`model`), each written the way its
framework recommends for speed.

- Transport bar (44 pt): a play button, the position (`model::position`), tempo "120 bpm".
- Ruler (28 pt) over the timeline: a bar number and a tick every bar, a small tick every beat.
- Track rows (64 pt each), scrolling down when they don't fit:
  - Header (176 pt) on the left: the track's colour bar, its name, M / S / R buttons
    (lit when on), and a thin meter on its right edge.
  - Timeline on the right: each clip a rounded rect in its track colour (dim fill, bright
    border), its name at the top left, and a preview of every note as a small rect (x by start,
    y by pitch in `PITCHES`, height 2 pt).
  - Bar lines down the timeline.
- Playhead: a 1 pt line over ruler and timeline.
- Mixer (232 pt) at the bottom: a strip (72 pt) per track and one for the master, with the
  name, a fader (track and thumb at `gain_db`), and a two-bar meter.

Scenarios (`model::Scenario`) drive it by themselves from the app's frame clock. Only what is
visible needs drawing. The timeline is wider than the window: scroll moves it, zoom scales x.
