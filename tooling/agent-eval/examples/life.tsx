import { adsr, Canvas, h, Knob, knob, note, sin, phasor, TAU, tool, type Shape, type Style } from "./sdk";

const COLUMNS = 16;
const ROWS = 8;
// C major pentatonic from C4 to E5, bottom row first.
const SCALE = [60, 62, 64, 67, 69, 72, 74, 76];

const CELL = 32;
const GAP = 3;
const WIDTH = COLUMNS * CELL;
const HEIGHT = ROWS * CELL;

type Life = {
  cells: boolean[][]; // cells[column][row], row 0 at the bottom
  cursor: number; // the column the sweep is in, with its fraction
  played: number; // the last column played, -1 for none
  sinceStep: number; // columns swept since the last generation
  running: boolean;
  generation: number;
};

const empty = () => Array.from({ length: COLUMNS }, () => Array<boolean>(ROWS).fill(false));

const seeded = () => {
  const cells = empty();
  // A glider and a blinker, so it plays from the start.
  for (const [c, r] of [[1, 5], [2, 4], [3, 4], [3, 5], [3, 6], [9, 2], [10, 2], [11, 2]]) cells[c][r] = true;
  return cells;
};

const step = (cells: boolean[][]) =>
  cells.map((column, c) =>
    column.map((alive, r) => {
      let around = 0;
      for (let dc = -1; dc <= 1; dc++)
        for (let dr = -1; dr <= 1; dr++)
          if ((dc || dr) && cells[(c + dc + COLUMNS) % COLUMNS][(r + dr + ROWS) % ROWS]) around++;
      return around === 3 || (alive && around === 2);
    }),
  );

const page: Style = { gap: 12, padding: 16, align: "center" };
const row: Style = { direction: "row", gap: 8, align: "center" };
const button: Style = { paddingX: 12, paddingY: 6, radius: 6, background: "#2a2f3a", color: "#e5e7eb" };

tool({
  name: "life",
  title: "Life",
  when: "You change the Game of Life toy: its speed, how often the grid evolves, or its bell sound",
  doc:
    "A Game of Life grid of 16 columns and 8 rows that plays itself. A cursor sweeps across it column by column and strikes the living cells of its column as one chord of soft bells: the bottom row is the lowest note, each row up is the next note of a C major pentatonic scale (C4 at the bottom, up to E5). The grid wraps at its edges and evolves on its own as the cursor moves. A click on a cell brings it to life or kills it; the page also has Pause, Clear and Random.\n\n" +
    "`speed` is how many columns the cursor sweeps a second. `evolve_every` is how many columns it sweeps between two generations: 16 is one generation a sweep, 1 a generation on every column. `ring` is how long a bell rings, `brightness` how much metallic shimmer it has at the strike, 0 a pure sine. `gain` is the level of each bell.",
  kind: "instrument",
  voices: 8,
  state: {
    speed: knob({ min: 0.5, max: 16, default: 4, unit: "hz", label: "Columns / s" }),
    evolve_every: knob({ min: 1, max: 32, default: 8, label: "Evolve every" }),
    ring: knob({ min: 200, max: 6000, default: 2500, unit: "ms", label: "Ring" }),
    brightness: knob({ min: 0, max: 1, default: 0.4, label: "Brightness" }),
    gain: knob({ min: 0, max: 1, default: 0.15, label: "Gain" }),
  },
  memory: (): Life => ({ cells: seeded(), cursor: 0, played: -1, sinceStep: 0, running: true, generation: 0 }),
  sound: ({ ring, brightness, gain }) => {
    // A soft FM bell: an inharmonic partial modulates the tone, and fades faster than it.
    const body = adsr(note.gate, 4, ring, 0, ring);
    const shimmer = adsr(note.gate, 1, ring.times(0.25), 0, ring.times(0.25));
    const modulator = sin(phasor(note.freq.times(3.5)).times(TAU)).times(brightness.times(shimmer).times(2.5));
    const tone = sin(phasor(note.freq).times(TAU).plus(modulator));
    const octave = sin(phasor(note.freq.times(2)).times(TAU)).times(0.15);
    return tone.plus(octave).times(body).times(note.velocity).times(gain);
  },
  tick: ({ state, memory, dt, play }) => {
    if (!memory.running) return;
    memory.cursor = (memory.cursor + (state.speed ?? 4) * dt) % COLUMNS;
    const column = Math.floor(memory.cursor);
    if (column === memory.played) return;
    memory.played = column;
    if (++memory.sinceStep >= Math.round(state.evolve_every ?? 8)) {
      memory.cells = step(memory.cells);
      memory.sinceStep = 0;
      memory.generation++;
    }
    const living = memory.cells[column].flatMap((alive, r) => (alive ? [r] : []));
    const velocity = 0.9 / Math.sqrt(Math.max(1, living.length));
    for (const r of living) play(SCALE[r], { seconds: 0.3, velocity });
  },
  page: ({ memory }) => {
    const shapes: Shape[] = [];
    const column = Math.floor(memory.cursor);
    shapes.push({ kind: "rect", x: column * CELL, y: 0, width: CELL, height: HEIGHT, color: "#1e293b" });
    memory.cells.forEach((cells, c) =>
      cells.forEach((alive, r) => {
        const lit = alive && c === column;
        shapes.push({
          kind: "rect",
          x: c * CELL + GAP / 2,
          y: (ROWS - 1 - r) * CELL + GAP / 2,
          width: CELL - GAP,
          height: CELL - GAP,
          radius: 5,
          color: lit ? "#fde68a" : alive ? "#38bdf8" : "#1f2937",
        });
      }),
    );
    const x = memory.cursor * CELL;
    shapes.push({ kind: "line", from: [x, 0], to: [x, HEIGHT], color: "#fbbf24", width: 2 });

    const toggle = (x: number, y: number) => {
      const c = Math.min(COLUMNS - 1, Math.floor(x * COLUMNS));
      const r = ROWS - 1 - Math.min(ROWS - 1, Math.floor(y * ROWS));
      memory.cells[c][r] = !memory.cells[c][r];
    };
    const randomize = () => {
      memory.cells = memory.cells.map((cells) => cells.map(() => Math.random() < 0.3));
      memory.generation = 0;
    };

    return (
      <div style={page}>
        <Canvas width={WIDTH} height={HEIGHT} background="#0f172a" shapes={shapes} onPress={toggle} />
        <div style={row}>
          <div style={button} onClick={() => (memory.running = !memory.running)}>
            {memory.running ? "Pause" : "Play"}
          </div>
          <div style={button} onClick={() => ((memory.cells = empty()), (memory.generation = 0))}>Clear</div>
          <div style={button} onClick={randomize}>Random</div>
          <div style={{ color: "#94a3b8", fontSize: 12 }}>Generation {memory.generation}</div>
        </div>
        <div style={row}>
          <Knob path="speed" label="Columns / s" min={0.5} max={16} default={4} unit="hz" />
          <Knob path="evolve_every" label="Evolve every" min={1} max={32} default={8} />
          <Knob path="ring" label="Ring" min={200} max={6000} default={2500} unit="ms" />
          <Knob path="brightness" label="Brightness" min={0} max={1} default={0.4} />
          <Knob path="gain" label="Gain" min={0} max={1} default={0.15} />
        </div>
      </div>
    );
  },
});
