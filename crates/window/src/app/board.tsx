// A clip in time, which `checks.test.tsx` drags with the test helpers and the window's tests
// with the pointer: both land it on the same numbers.
import { h, input, live, number, text, tool, type Pointer } from "./sdk";

/** What the handlers of the clip heard. */
export const heard: Pointer[] = [];
export const board = tool({
  name: "board", title: "Board", when: "Never", doc: "A clip in time.", sound: () => input,
  state: { start: number({ min: 0, max: 100000, default: 480 }), name: text({ default: "" }) },
  actions: { home: { title: "Back to the start", keys: ["h"], run: ({ update }) => update("Home", (draft) => void (draft.state.start = 0)) } },
  view: (self, { update, confirm, shortcut }) => (
    <box key="lane" label="Lane" w={400} h={40} plane={{ x: 0.1 }} onDrop={(d) => update("Drop", (draft) => void (draft.state.name = d.audio[0] ?? ""))}>
      <box key="clip" label="Clip" place={[live.drag("clip").dx.plus(self.state.start), 0, 960, 40]} drag={{ x: [-self.state.start, 100000], snap: [240, 1] }}
        onPress={(p) => heard.push(p)} onDragEnd={(p) => { heard.push(p); update("Move", (draft) => void (draft.state.start += p.dx)); }}>
        <col overlay={{ open: "menu" }} role="menu">
          <box key="delete" label="Delete" onPress={async () => { if (await confirm({ message: "Delete the clip?", confirm: "Delete" })) update("Delete", (draft) => void (draft.state.start = 0)); }} />
        </col>
      </box>
      <text key="key">{shortcut("home") ?? "none"}</text>
      <field key="name" label="Name" value={self.state.name} onCommit={(name) => update("Name", (draft) => void (draft.state.name = name ?? "left"))} />
    </box>
  ),
});
