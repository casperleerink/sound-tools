# Meter

Hears a sound on its input `audio` and keeps its level and its last sound, for its parent's view. It plays nothing on. Its record holds nothing: `{"tool": "meter", "state": {}}`.

Its parent connects what it shows: `{ from: { child: "bass-mix" }, to: { child: "bass-meter" } }`. The view reads it by its path: `now.peak("bass-meter")`, the loudest sample since the last frame, 0 to 1 and above when it clips, and `now.scope("bass-meter")`, its last sound for a `Wave`. A meter inside a child is `"bass/meter"`.
