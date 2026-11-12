# Meter

Hears a sound on its input `audio` and keeps its level for its parent's view, at a place only a connection names, such as before a fader. A view draws the sound of any child with no meter: `now.scope("bass")`. It plays nothing on. Its record holds nothing: `{"tool": "meter", "state": {}}`.

Its parent connects what it shows: `{ from: { child: "bass-mix" }, to: { child: "bass-meter" } }`. A view reads its level with `now.peak("bass-meter")`.
