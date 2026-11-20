import { h, live, type Card, type Dropped } from "../../../sdk";
import theme from "../../../theme.json";
import { Button } from "../../button";
import { Cell } from "../../cell";
import { binder } from "../../field";
import { Knob } from "../../knob";
import { Pad } from "../../pad";
import { Select } from "../../select";
import { Toggle } from "../../toggle";
import { displayWidth, DISPLAY_HEIGHT } from "../../display";
import { DeviceCard } from "../device-card";

/** Pads across and up; the first pad's note; points between pads (space 1). */
const [ACROSS, FIRST_NOTE, GAP] = [4, 36, 4];
const PAD_W = (displayWidth(6) - (ACROSS - 1) * GAP) / ACROSS;
const PAD_H = (DISPLAY_HEIGHT - (ACROSS - 1) * GAP) / ACROSS;
/** How hard a press on a pad plays it, as a key at 100 of 127 would. */
const PRESS = 100 / 127;
/** What the Sound list says for the pad's own file and for the file panel. */
const [SAMPLE, CHOOSE] = ["sample", "choose"];
type PadNumber = "volume_db" | "pitch_semitones" | "decay_ms" | "pan";

/**
 * The Drum pad: its 16 pads as the display, the bottom left the first, each named for what it
 * plays and lit while it sounds; a press plays it and chooses it, a file dropped on it makes it
 * play that file. Beside them the sound of the chosen pad and its volume, pitch, decay and pan;
 * behind More its choke switch and a way back to the kit.
 */
export const card: Card<"drum-pad", { expanded: boolean; pad: number }> = (self, ctx) => {
  const field = binder(self, ctx);
  const { memory } = ctx;
  const pads = self.facts.pads ?? [];
  const chosen = Math.min(Math.max(memory.pad ?? 0, 0), Math.max(pads.length - 1, 0));
  const pad = pads[chosen];
  const note = String(FIRST_NOTE + chosen);
  const record = (self.state.pads as Record<string, Record<string, unknown>> | undefined)?.[note] ?? {};
  const choose = (index: number) => (memory.pad = index);
  /** Changes pad `index` as one step; `null` takes a field out of its record. */
  const setPad = (label: string, index: number, values: Record<string, unknown>) =>
    ctx.update(label, (draft) => {
      const all = ((draft.state as { pads?: Record<string, Record<string, unknown>> }).pads ??= {});
      const key = String(FIRST_NOTE + index);
      const changed = { ...all[key], ...values };
      for (const [name, value] of Object.entries(changed)) if (value === null) delete changed[name];
      all[key] = changed;
    });
  /** A file brought into the project plays on pad `index` from its start to its end, at its own pitch. */
  const load = (index: number, file: string) => {
    setPad("Load sample", index, { sample: file, sound: null, pitch_semitones: 0, decay_ms: 10000 });
    choose(index);
  };
  const chooseFile = async (index: number) => {
    const file = await ctx.openFile({ title: "Choose a sound for the pad" });
    if (file !== null) load(index, file);
  };

  const grid = (
    <col key="pads" label={`${field.title} pads`} role="group" group gap={1}>
      {[3, 2, 1, 0].map((row) => (
        <row gap={1}>
          {[0, 1, 2, 3].map((column) => {
            const index = row * ACROSS + column;
            const each = pads[index];
            const file = each?.file;
            return (
              <Pad id={`pad-${FIRST_NOTE + index}`} label={`${field.title} pad ${index + 1}`} text={each?.name ?? `Pad ${index + 1}`} pitch={FIRST_NOTE + index}
                level={live.feed(`levels.${index}`)} sample={file !== null && file !== undefined} missing={file?.state === "missing" || file?.state === "does_not_play"}
                chosen={index === chosen} w={PAD_W} h={PAD_H}
                onPress={() => {
                  choose(index);
                  ctx.play(FIRST_NOTE + index, { velocity: PRESS });
                }}
                onDrop={(dropped: Dropped) => dropped.audio[0] !== undefined && load(index, dropped.audio[0])} />
            );
          })}
        </row>
      ))}
    </col>
  );
  if (!pad) return <DeviceCard id="card" title={field.title} display={grid} />;

  /** A number of the chosen pad: what plays, its kit's value on a reset, the sound following a drag. */
  const knob = (name: PadNumber, text: string, unit: "db" | "semitones" | "ms" | "pan") => {
    const bound = field.number(`pads.${note}.${name}`, text);
    const value = typeof record[name] === "number" ? (record[name] as number) : pad[name];
    return { ...bound, unit, value, initial: pad.kit[name], onChange: (to: number) => setPad(`Change ${text.toLowerCase()}`, chosen, { [name]: to }) };
  };
  // The record first: the facts follow an edit a moment later.
  const file = typeof record.sample === "string" ? record.sample : typeof record.sound === "string" ? undefined : pad.file?.name;
  const sound = file !== undefined ? SAMPLE : ((record.sound as string | undefined) ?? pad.sound ?? "");
  const sounds = [
    ...(self.facts.sounds ?? []).map((each) => ({ value: each.value as string, text: each.name })),
    ...(file !== undefined ? [{ value: SAMPLE, text: file }] : []),
    { value: CHOOSE, text: "Choose file…" },
  ];
  const pick = (to: string) => {
    if (to === CHOOSE) void chooseFile(chosen);
    else if (to !== SAMPLE) setPad("Change sound", chosen, { sound: to, sample: null });
  };
  const choke = pad.choke;
  // The sound heads the knobs: it says which pad they turn.
  const head = (
    <col w={2 * theme.size.cell} gap={1}>
      <Cell span={2} text="Sound"><Select id="sound" label={`${field.title} sound`} options={sounds} value={sound} w={2 * theme.size.cell - 8} menuW={200} onChange={pick} /></Cell>
      <row><Knob {...knob("pitch_semitones", "Pitch", "semitones")} bipolar /><Knob {...knob("decay_ms", "Decay", "ms")} /></row>
    </col>
  );
  return (
    <DeviceCard id="card" title={field.title} display={grid}
      cells={[head, <Knob {...knob("volume_db", "Volume", "db")} />, <Knob {...knob("pan", "Pan", "pan")} bipolar />]}
      more={[
        <Cell><Toggle id="choke" label={`${field.title} choke`} text="Choke" on={choke} tip="A hit of a pad in the choke group cuts the others" onToggle={() => setPad(choke ? "Take out of choke group" : "Add to choke group", chosen, { choke: !choke })} /></Cell>,
        <Cell><Button id="reset-pad" label={`${field.title} reset pad`} text="Reset" size="sm" tip="Back to the kit's sound and settings" onPress={() => ctx.update("Reset pad", (draft) => void delete (draft.state as { pads?: Record<string, unknown> }).pads?.[note])} /></Cell>,
      ]}
      expanded={(memory.expanded ?? false)} onExpand={() => (memory.expanded = !memory.expanded)} />
  );
};
