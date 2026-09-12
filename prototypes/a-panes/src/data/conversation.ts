/** Fake agent conversation for the sidebar. Three end states share the same start. */

export type BuildState = "idle" | "building" | "failed";

export type ToolCall =
  | { kind: "read"; path: string }
  | { kind: "edit"; path: string; summary: string; lines?: { added: number; removed: number } }
  | { kind: "create"; path: string; summary: string; lines?: { added: number; removed: number } }
  | { kind: "connect"; summary: string }
  | { kind: "build"; extension: string; status: "ok" | "failed" | "running"; seconds?: number; output: string[] }
  | { kind: "reload"; summary: string };

export type Message =
  | { id: string; role: "user"; text: string; time: string }
  | { id: string; role: "agent"; time: string; blocks: Block[]; working?: boolean };

export type Block = { kind: "text"; text: string } | { kind: "tools"; calls: ToolCall[] } | { kind: "status"; text: string; tone: "ok" | "warn" | "error" | "info" };

const okBuildOutput = [
  "   Compiling rhythm-loops v0.1.0 (extensions/rhythm-loops)",
  "   Compiling three-voices-runtime v0.1.0 (.runtime)",
  "    Finished `dev` profile [optimized + debuginfo] target(s) in 1.34s",
  "  Replacing runtime: waiting for project writes… done",
  "  Runtime ready in 2.21s",
];

const runningBuildOutput = [
  "   Compiling rhythm-loops v0.1.0 (extensions/rhythm-loops)",
];

const failedBuildOutput = [
  "   Compiling rhythm-loops v0.1.0 (extensions/rhythm-loops)",
  "error[E0308]: mismatched types",
  "   --> extensions/rhythm-loops/src/lib.rs:142:18",
  "    |",
  "142 |         offset + self.swing",
  "    |                  ^^^^^^^^^^ expected `f64`, found `Param<f64>`",
  "    |",
  "help: read the parameter value first: `self.swing.value()`",
  "",
  "error: could not compile `rhythm-loops` (lib) due to 1 previous error",
];

const base: Message[] = [
  {
    id: "u1",
    role: "user",
    time: "14:02",
    text: "Give these three voices independent rhythms and let me stretch each pattern by dragging it.",
  },
  {
    id: "a1",
    role: "agent",
    time: "14:02",
    blocks: [
      { kind: "text", text: "I'll look at how the three Tone instances are triggered right now." },
      {
        kind: "tools",
        calls: [
          { kind: "read", path: "project.json" },
          { kind: "read", path: "state/tone-a.json" },
          { kind: "read", path: "state/tone-b.json" },
          { kind: "read", path: "state/tone-c.json" },
        ],
      },
      {
        kind: "text",
        text: "All three share one pulse from the transport. I'll write a small Rhythm Loops extension: one pattern per voice, each with its own length and rate. You stretch a pattern by dragging its right edge, so the voices can drift apart and realign.",
      },
      {
        kind: "tools",
        calls: [
          {
            kind: "create",
            path: "extensions/rhythm-loops/src/lib.rs",
            summary: "Loops tool: 3 voices, pattern bars, stretch by drag",
            lines: { added: 218, removed: 0 },
          },
          { kind: "create", path: "extensions/rhythm-loops/Cargo.toml", summary: "Extension manifest", lines: { added: 12, removed: 0 } },
          { kind: "edit", path: "project.json", summary: "Enable rhythm-loops", lines: { added: 1, removed: 0 } },
          { kind: "build", extension: "rhythm-loops", status: "ok", seconds: 2.21, output: okBuildOutput },
          { kind: "reload", summary: "Playback stopped, project restored" },
        ],
      },
      {
        kind: "tools",
        calls: [
          { kind: "create", path: "state/loops.json", summary: "Loops instance with 3 voices (8, 5 and 12 steps)", lines: { added: 41, removed: 0 } },
          { kind: "connect", summary: "Loops voice 1–3 → Tone A, B, C trigger" },
        ],
      },
      {
        kind: "text",
        text: "Done. Loops is open in the workspace. Each voice has its own bar; drag its right edge to stretch it. Voice 2 is 5 steps against 8 and 12, so all three only realign every 120 steps.",
      },
    ],
  },
  { id: "u2", role: "user", time: "14:09", text: "Make the second voice a bit slower and quieter." },
  {
    id: "a2",
    role: "agent",
    time: "14:09",
    blocks: [
      {
        kind: "tools",
        calls: [
          { kind: "edit", path: "state/loops.json", summary: "voices[1].rate 1.0 → 0.8", lines: { added: 1, removed: 1 } },
          { kind: "edit", path: "state/tone-b.json", summary: "level −6 → −12 dB", lines: { added: 1, removed: 1 } },
        ],
      },
      {
        kind: "text",
        text: "Voice 2 now runs at 0.8× and Tone B sits at −12 dB. These are project edits, so no rebuild was needed and playback kept going.",
      },
    ],
  },
];

const swingRequest: Message = {
  id: "u3",
  role: "user",
  time: "14:15",
  text: "Add a swing amount to each pattern, I want to push the off-beats late.",
};

export const CONVERSATIONS: Record<BuildState, Message[]> = {
  idle: base,
  building: [
    ...base,
    swingRequest,
    {
      id: "a3",
      role: "agent",
      time: "14:15",
      working: true,
      blocks: [
        { kind: "text", text: "Adding a swing parameter per voice. Off-beat steps get delayed by swing × half a step." },
        {
          kind: "tools",
          calls: [
            { kind: "edit", path: "extensions/rhythm-loops/src/lib.rs", summary: "Add swing param and offset", lines: { added: 26, removed: 4 } },
            { kind: "build", extension: "rhythm-loops", status: "running", output: runningBuildOutput },
          ],
        },
        { kind: "status", tone: "info", text: "Building rhythm-loops… playback keeps running until the reload." },
      ],
    },
  ],
  failed: [
    ...base,
    swingRequest,
    {
      id: "a3",
      role: "agent",
      time: "14:15",
      working: true,
      blocks: [
        { kind: "text", text: "Adding a swing parameter per voice. Off-beat steps get delayed by swing × half a step." },
        {
          kind: "tools",
          calls: [
            { kind: "edit", path: "extensions/rhythm-loops/src/lib.rs", summary: "Add swing param and offset", lines: { added: 26, removed: 4 } },
            { kind: "build", extension: "rhythm-loops", status: "failed", seconds: 0.9, output: failedBuildOutput },
          ],
        },
        { kind: "status", tone: "error", text: "Build failed. The previous version of rhythm-loops is still running." },
        { kind: "text", text: "Type mismatch in the swing offset: I used the parameter handle instead of its value. Fixing it now." },
        {
          kind: "tools",
          calls: [{ kind: "edit", path: "extensions/rhythm-loops/src/lib.rs", summary: "Read swing value before adding", lines: { added: 1, removed: 1 } }],
        },
      ],
    },
  ],
};
