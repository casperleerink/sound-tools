/* Fake agent conversation. One story, three endings (default / building / failed). */
import type { DevState } from "./dev-state";

export type ToolCallKind = "read" | "edit-record" | "edit-source" | "build" | "reload";
export type ToolCallStatus = "done" | "running" | "failed" | "pending";

export interface ToolCall {
  id: string;
  kind: ToolCallKind;
  /** Short label: "Read", "Edit", "Edit source", "Build", "Reload". */
  label: string;
  /** File path or target, shown in mono. */
  target: string;
  status: ToolCallStatus;
  /** Right-aligned meta: "12 ms", "+84 −12", "2.2 s". */
  meta?: string;
  /** Extra note under the row, e.g. "playback stopped" or "no build needed". */
  note?: string;
  /** Build output when failed. */
  output?: string;
  /** 0..1 while running (build). */
  progress?: number;
}

export type MessagePart =
  | { type: "text"; text: string }
  | { type: "tools"; calls: ToolCall[] }
  | { type: "working"; text: string };

export interface Message {
  id: string;
  role: "user" | "agent";
  time: string;
  parts: MessagePart[];
}

const request: Message = {
  id: "m1",
  role: "user",
  time: "14:02",
  parts: [{ type: "text", text: "Give these three voices independent rhythms and let me stretch each pattern by dragging it." }],
};

const plan: MessagePart = {
  type: "text",
  text: "I'll turn the step sequencer into a polyrhythm tool: each voice gets its own cycle length and a stretch handle on its pattern bar.",
};

const readAndEdit: ToolCall[] = [
  { id: "t1", kind: "read", label: "Read", target: "state/three-voices.json", status: "done", meta: "12 ms" },
  { id: "t2", kind: "read", label: "Read", target: "extensions/step-seq/src/lib.rs", status: "done", meta: "9 ms" },
  { id: "t3", kind: "edit-source", label: "Edit source", target: "extensions/step-seq/src/lib.rs", status: "done", meta: "+84 −12" },
  { id: "t4", kind: "edit-source", label: "Edit source", target: "extensions/step-seq/src/view.rs", status: "done", meta: "+61 −9" },
];

const buildOk: ToolCall = { id: "t5", kind: "build", label: "Build", target: "polyrhythm", status: "done", meta: "2.2 s" };
const reloadOk: ToolCall = { id: "t6", kind: "reload", label: "Reload", target: "project runtime", status: "done", meta: "0.4 s", note: "Playback stopped and project restored" };

const buildRunning: ToolCall = { id: "t5", kind: "build", label: "Building polyrhythm…", target: "extensions/step-seq", status: "running", meta: "1.4 s", progress: 0.62 };
const reloadPending: ToolCall = { id: "t6", kind: "reload", label: "Reload", target: "project runtime", status: "pending", note: "Waits for the build; stops playback" };

const buildFailed: ToolCall = {
  id: "t5",
  kind: "build",
  label: "Build failed",
  target: "polyrhythm",
  status: "failed",
  meta: "1.8 s",
  note: "Previous version is still running",
  output: `error[E0308]: mismatched types
  --> extensions/step-seq/src/lib.rs:142:31
    |
142 |         let len = voice.steps * stretch;
    |                               ^^^^^^^ expected \`u32\`, found \`f32\`
    |
help: convert the value with \`as f32\`

error: could not compile \`step-seq\` (lib) due to 1 previous error`,
};

export function conversationFor(state: DevState): Message[] {
  if (state === "building") {
    return [
      request,
      {
        id: "m2",
        role: "agent",
        time: "14:02",
        parts: [plan, { type: "tools", calls: [...readAndEdit, buildRunning, reloadPending] }, { type: "working", text: "Waiting for the build. You can keep playing the current version." }],
      },
    ];
  }
  if (state === "failed") {
    return [
      request,
      {
        id: "m2",
        role: "agent",
        time: "14:02",
        parts: [
          plan,
          { type: "tools", calls: [...readAndEdit, buildFailed] },
          { type: "text", text: "The build failed on a type mismatch in the stretch code. Your previous version is still running, so nothing changed for you yet. Fixing it now." },
          {
            type: "tools",
            calls: [{ id: "t7", kind: "edit-source", label: "Edit source", target: "extensions/step-seq/src/lib.rs", status: "running", meta: "line 142" }],
          },
        ],
      },
    ];
  }
  return [
    request,
    {
      id: "m2",
      role: "agent",
      time: "14:02",
      parts: [
        plan,
        { type: "tools", calls: [...readAndEdit, buildOk, reloadOk] },
        {
          type: "text",
          text: "Done. A keeps 4 steps over 2.0 s, B has 5 over 3.0 s and C has 3 over 2.4 s, so it all repeats every 12 s. Drag a bar's right edge to stretch it; the Voices view takes exact lengths.",
        },
      ],
    },
    { id: "m3", role: "user", time: "14:05", parts: [{ type: "text", text: "Make voice B a bit shorter, 2.5 seconds." }] },
    {
      id: "m4",
      role: "agent",
      time: "14:05",
      parts: [
        {
          type: "tools",
          calls: [{ id: "t8", kind: "edit-record", label: "Edit", target: "state/three-voices.json", status: "done", meta: "voices[1].length", note: "Project edit, applied live, no build" }],
        },
        { type: "text", text: "Voice B is 2.5 s now. That was a project edit, so it applied while playing. The voices line up every 60 s." },
      ],
    },
  ];
}

export interface BuildStatus {
  state: "idle" | "building" | "failed" | "reloaded";
  label: string;
  detail?: string;
  progress?: number;
}

export function buildStatusFor(state: DevState): BuildStatus {
  if (state === "building") return { state: "building", label: "Building polyrhythm…", detail: "1.4 s", progress: 0.62 };
  if (state === "failed") return { state: "failed", label: "Build failed", detail: "previous version running" };
  return { state: "reloaded", label: "Reloaded", detail: "2.2 s build" };
}
