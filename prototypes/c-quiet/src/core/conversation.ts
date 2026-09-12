/* Fake agent conversation. One story, three endings (default / building / failed).
   A turn is the composer's message, one activity line, and the agent's result. */
import type { DevState } from "./dev-state";

export type StepKind = "read" | "edit" | "build" | "reload";

export interface Step {
  kind: StepKind;
  /** "Read", "Edit", "Build", "Reload". */
  label: string;
  /** File path or target, shown in mono. */
  target: string;
  /** Right-aligned note: "2.2 s", "applied live", "failed". */
  note?: string;
}

export type Activity =
  /** The agent is busy: one line with a slow pulse. */
  | { state: "working"; label: string }
  /** The build failed; the agent's text says why in one sentence. */
  | { state: "failed"; label: string }
  /** The turn is over. Collapses to "Worked for 12 s". */
  | { state: "done"; seconds: number };

export interface Turn {
  id: string;
  request: string;
  activity: Activity;
  /** Expanded on click only. */
  history: Step[];
  /** Agent's result text. Absent while working. */
  result?: string;
}

const request = "Give these three voices independent rhythms and let me stretch each pattern by dragging it.";

const readAndEdit: Step[] = [
  { kind: "read", label: "Read", target: "state/three-voices.json" },
  { kind: "read", label: "Read", target: "extensions/step-seq/src/lib.rs" },
  { kind: "edit", label: "Edit", target: "extensions/step-seq/src/lib.rs" },
  { kind: "edit", label: "Edit", target: "extensions/step-seq/src/view.rs" },
];

export function conversationFor(state: DevState): Turn[] {
  if (state === "building") {
    return [
      {
        id: "t1",
        request,
        activity: { state: "working", label: "Building Polyrhythm" },
        history: [...readAndEdit, { kind: "build", label: "Build", target: "polyrhythm", note: "running" }],
      },
    ];
  }
  if (state === "failed") {
    return [
      {
        id: "t1",
        request,
        activity: { state: "failed", label: "Build failed" },
        history: [
          ...readAndEdit,
          { kind: "build", label: "Build", target: "polyrhythm", note: "failed, previous version still running" },
        ],
        result: "The stretch code has a type error on line 142. Fixing it now.",
      },
    ];
  }
  return [
    {
      id: "t1",
      request,
      activity: { state: "done", seconds: 12 },
      history: [
        ...readAndEdit,
        { kind: "build", label: "Build", target: "polyrhythm", note: "2.2 s" },
        { kind: "reload", label: "Reload", target: "project", note: "playback restarted" },
      ],
      result: "Done. Each voice has its own length now: A 2.0 s, B 3.0 s, C 2.4 s. Drag a bar's right edge to stretch it.",
    },
    {
      id: "t2",
      request: "Make voice B a bit shorter, 2.5 seconds.",
      activity: { state: "done", seconds: 2 },
      history: [{ kind: "edit", label: "Edit", target: "state/three-voices.json", note: "applied live" }],
      result: "Voice B is 2.5 s now. The voices line up every 60 s.",
    },
  ];
}
