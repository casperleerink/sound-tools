# Meter

Hears a sound on its input `audio` and keeps its level and its last sound for its parent's view. It plays nothing on. Its record holds nothing: `{"tool": "meter", "state": {}}`.

Its parent connects what it shows: `{ from: { child: "bass-mix" }, to: { child: "bass-meter" } }`. A view reads its level with `now.peak("bass-meter")` and its last sound with `now.scope("bass-meter")`.
