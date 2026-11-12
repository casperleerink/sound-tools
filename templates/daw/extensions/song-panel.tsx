import { Button, DeviceCard, For, Fragment, h, Knob, memo, Select, Show, Text, Toggle, type Accessor, type CardSelect } from "./sdk";
import { returnTitle, sendsOf, Volume } from "./song-mixer";
import { colourOf, HEADER, inputOf, itemTool, Mark, muted, nameFrom, titleOf, type Page } from "./song-page";
import { freeName } from "./track";

// What shows under the timeline for one track or return, its panel, and for the master.

// The panel of the chosen track, as tall as a card and the room around it.
/** A device card's height, and the panel's: its cards, 12 points above them and under. */
const CARD = 192;
const RACK = CARD + 24;
// The fader of the chosen track's panel.
const PANEL_FADER = 112;
/** The top of a panel's first column: its mark, its name and its close button. */
const TITLE = { height: 32, direction: "row", align: "center", gap: "sm", paddingLeft: "lg", paddingRight: "sm" } as const;
/** A fader over its value, under the title. */
const VOLUME = { align: "center", gap: "sm", paddingTop: "xs" } as const;
/** The cards of a panel, side by side. */
const CARDS = { grow: true, direction: "row", gap: "md", paddingX: "lg", paddingTop: "md", align: "start" } as const;
/** A surface as tall as a device card and with its look, for what a panel shows beside them. */
const SURFACE = { height: CARD, radius: "lg", background: "surface", borderColor: "border", borderWidth: 1, paddingX: "lg", keep: true } as const;
/** Where on a track a sidechain takes its sound, and what its select says, as the arrangement's. */
const TAPS = [["pre_fx", "Pre FX"], ["post_fx", "Post FX"], ["post_fader", "Post mixer"]] as const;
type Tap = (typeof TAPS)[number][0];

/** A device the composer picks from what the window offers that takes `takes`, as the window's pickers list them. */
async function pick({ offers, openMenu }: Page, takes: "notes" | "audio", at?: string) {
  const { offers: offered, notes, here } = await offers(takes, { at });
  const items = offered.map(({ key, name, detail, group, icon, unavailable }) => ({ label: name, detail: detail ?? undefined, group, icon, checked: key === here, disabled: unavailable !== null }));
  const picked = await openMenu(items, { notes, width: 280, maxHeight: 540, search: true });
  return picked === null ? undefined : offered[picked];
}

/**
 * The devices of the track or return `name`: an instrument, which can be changed, or the card
 * of an audio track's chosen region, then the effects of its chain, which can be added, turned
 * off, removed and dragged into another order. They scroll on their own: a track's sends stay
 * at the right, however long its chain is.
 */
export const Rack = ({ page, name }: { page: Page; name: string }) => {
  const { state, children, memory, update, addOffer, rows, strips, toggleMute, toggleSolo } = page;
  const chainOf = () => {
    const row = strips().find((each) => each.name === name);
    const effects = row?.tool === "track" || row?.tool === "tape" || row?.tool === "bus" ? row.children.find((child) => child.name === "chain") : undefined;
    return effects?.tool === "chain" ? effects : undefined;
  };
  const order = () => chainOf()?.state.order ?? [];
  const isOff = (effect: string) => chainOf()?.state.off.includes(effect) ?? false;
  /** Changes the chain of the track as one undo step `label`. */
  const editChain = (label: string, change: (effects: { state: { order: string[]; off: string[] }; children: { name: string }[] }) => void) =>
    update(label, ({ children }) => {
      const row = children.find((child) => child.name === name);
      const effects = row?.tool === "track" || row?.tool === "tape" || row?.tool === "bus" ? row.children.find((child) => child.name === "chain") : undefined;
      if (effects?.tool === "chain") change(effects);
    });
  const addEffect = async () => {
    const offer = await pick(page, "audio");
    if (!offer) return;
    const taken = chainOf()?.children.map((child) => child.name) ?? [];
    const effect = freeName(taken, nameFrom(offer.key) || "effect");
    addOffer(offer.key, `${name}/chain/${effect}`, "Add an effect");
    editChain("Add an effect", (effects) => (effects.state.order = [...effects.state.order, effect]));
  };
  /** Another device in place of the one at `path`: an instrument from what takes notes, an effect from what takes sound. */
  const replace = async (takes: "notes" | "audio", path: string, label: string) => {
    const offer = await pick(page, takes, path);
    if (!offer) return;
    unkey(path, label);
    addOffer(offer.key, path, label);
  };
  /** Takes the sidechain off the effect at `path`, as part of the step `label`: it goes, or another device takes its place. */
  const unkey = (path: string, label: string) => {
    if (state.sidechains.some((each) => each.effect === path)) update(label, (song) => (song.state.sidechains = song.state.sidechains.filter((each) => each.effect !== path)));
  };
  const remove = (effect: string) => {
    unkey(`${name}/chain/${effect}`, "Remove an effect");
    editChain("Remove an effect", (effects) => {
      effects.state.order = effects.state.order.filter((each) => each !== effect);
      effects.state.off = effects.state.off.filter((each) => each !== effect);
      effects.children = effects.children.filter((child) => child.name !== effect);
    });
  };
  /** The sidechain picker of an effect, as the arrangement's: Off or a track, and where on another track. */
  const sidechainOf = (effect: string): CardSelect[] => {
    const path = `${name}/chain/${effect}`;
    const keyed = state.sidechains.find((each) => each.effect === path);
    const source: CardSelect<string | null> = {
      label: "Sidechain",
      value: keyed?.from ?? null,
      options: [{ value: null, label: "Off" }, ...rows().map((each) => ({ value: each.name, label: titleOf(each) }))],
      onChange: (from) =>
        update(from === null ? "Turn off sidechain" : keyed ? "Change sidechain" : "Turn on sidechain", (song) => {
          const rest = song.state.sidechains.filter((each) => each.effect !== path);
          // Its own track keys it before its effects, else it would key itself; another keeps the tap, or starts after its effects.
          const tap = from === name ? "pre_fx" : (keyed?.tap ?? "post_fx");
          song.state.sidechains = from === null ? rest : [...rest, { effect: path, from, tap }];
        }),
    };
    if (!keyed || keyed.from === name) return [source];
    const tap: CardSelect<Tap> = {
      label: "Tap",
      value: keyed.tap,
      options: TAPS.map(([value, label]) => ({ value, label })),
      onChange: (tap) => update("Change sidechain tap", (song) => (song.state.sidechains = song.state.sidechains.map((each) => (each.effect === path ? { ...each, tap } : each)))),
    };
    return [source, tap];
  };
  const toggle = (effect: string) =>
    editChain(isOff(effect) ? "Turn an effect on" : "Turn an effect off", (effects) => {
      effects.state.off = effects.state.off.includes(effect) ? effects.state.off.filter((each) => each !== effect) : [...effects.state.off, effect];
    });
  // An effect dragged onto another goes before it, onto the instrument first, onto Add effect last.
  const moveBefore = (moved: string, before: string | null) =>
    order().includes(moved) && moved !== before &&
    editChain("Move an effect", (effects) => {
      const rest = effects.state.order.filter((each) => each !== moved);
      rest.splice(before === null ? rest.length : rest.indexOf(before), 0, moved);
      effects.state.order = rest;
    });
  const row = () => strips().find((each) => each.name === name);
  const isReturn = () => row()?.tool === "bus";
  const sends = () => {
    const sending = row();
    return sending ? sendsOf(sending) : [];
  };
  return (
    <div style={{ height: RACK, direction: "row", keep: true }}>
      <div style={{ width: HEADER - 1, keep: true, paddingTop: "md" }}>
        <div style={TITLE}>
          <Mark colour={() => (row() ? colourOf(row()!) : "blue")} />
          <Text variant="heading" style={{ grow: true, truncate: true }}>{() => (row() ? titleOf(row()!) : name)}</Text>
          <Button key="close-panel" icon="x" variant="ghost" size="sm" tooltip="Close" shortcut="song.back" onClick={() => (memory.below = null)} />
        </div>
        <div style={{ direction: "row", paddingLeft: "sm", gap: "lg" }}>
          <div style={VOLUME}>
            {() => <Volume path={`${name}/mix`} length={PANEL_FADER} meter={memory.armed.has(name) ? { input: inputOf(row()) } : { of: `${name}/mix` }} />}
          </div>
          <div style={{ align: "center" }}>
            <Knob field={`${name}/mix.pan`} label="Pan" min={-1} max={1} default={0} unit="pan" bipolar />
            <div style={{ direction: "row", gap: "xs", paddingTop: "sm" }}>
              <Toggle key={`panel-mute-${name}`} size="sm" label="M" tooltip="Mute" on={() => (row() ? muted(row()!) : false)} color="peach" onClick={() => toggleMute(name)} />
              <Show when={() => !isReturn()}>
                {() => <Toggle key={`panel-solo-${name}`} size="sm" label="S" tooltip="Solo" on={() => state.solo.includes(name)} color="yellow" onClick={() => toggleSolo(name)} />}
              </Show>
            </div>
            <Show when={() => row()?.tool === "tape"}>{() => <div style={{ paddingTop: "md" }}><InputSelect page={page} name={name} /></div>}</Show>
            <Show when={isReturn}>
              {() => (
                <div style={{ paddingTop: "md", gap: "xs", align: "center" }}>
                  <Text variant="label" tone="muted">Sends</Text>
                  <PreFader page={page} name={name} />
                </div>
              )}
            </Show>
          </div>
        </div>
      </div>
      <div style={{ width: 1, background: "border", keep: true }} />
      <div key="rack" style={{ ...CARDS, scroll: "x" }}>
        {() =>
          row()?.tool === "tape" ? (
            <>
              {() => {
                const chosen = [...memory.selected].find((key) => key.startsWith(`${name}/`) && itemTool(children, key) === "region");
                return chosen ? <DeviceCard key={`clip-${chosen}`} path={chosen} /> : <EmptyClip />;
              }}
              <div style={{ width: 1, height: CARD, background: "border", keep: true }} />
            </>
          ) : row()?.tool === "track" ? (
            <div key="instrument" onDrop={({ carried }) => carried && order()[0] !== undefined && moveBefore(carried, order()[0]!)}>
              <DeviceCard path={`${name}/synth`} onTitle={() => replace("notes", `${name}/synth`, "Change the instrument")} />
            </div>
          ) : null
        }
        <For each={order}>
          {(effect) => (
            <div key={`effect-${effect}`} carry={effect} onDrop={({ carried }) => carried && moveBefore(carried, effect)}>
              <DeviceCard path={`${name}/chain/${effect}`} on={() => !isOff(effect)} onPower={() => toggle(effect)} onClose={() => remove(effect)}
                onTitle={() => replace("audio", `${name}/chain/${effect}`, "Change an effect")}
                sidechain={() => sidechainOf(effect)} />
            </div>
          )}
        </For>
        <div key="add-effect" style={{ keep: true }} onDrop={({ carried }) => carried && moveBefore(carried, null)}>
          <Button key="add-effect" label="Add effect" icon="plus" variant="ghost" color="text-secondary" size="lg" onClick={addEffect} />
        </div>
      </div>
      <Show when={() => sends().length > 0}>
        {() => (
          <>
            <div style={{ width: 1, background: "border", keep: true }} />
            <Sends page={page} name={name} sends={sends} />
          </>
        )}
      </Show>
    </div>
  );
};

/** The first card of an audio track while none of its regions is chosen, as a device card looks. */
const EmptyClip = () => (
  <div key="clip-empty" style={{ ...SURFACE, width: 120 }}>
    <div style={{ height: 32, justify: "center" }}>
      <Text variant="heading">Clip</Text>
    </div>
    <Text variant="label">Select a clip.</Text>
  </div>
);
/** The channels of the input an audio track records, each alone and then the pair. */
const INPUTS = [{ label: "In 1", channels: [0] }, { label: "In 2", channels: [1] }, { label: "In 1 + 2", channels: [0, 1] }];
const InputSelect = ({ page: { update, rows }, name }: { page: Page; name: string }) => {
  const channels = () => {
    const owner = rows().find((each) => each.name === name);
    const rec = owner?.tool === "tape" ? owner.children.find((child) => child.name === "rec") : undefined;
    return rec?.tool === "recorder" ? rec.state.channels : [];
  };
  return (
    <Select key={`input-${name}`} size="sm" title="Input" value={() => Math.max(INPUTS.findIndex((each) => each.channels.join() === channels().join()), 0)}
      options={INPUTS.map(({ label }, index) => ({ value: index, label }))}
      onChange={(index) =>
        update("Change input", ({ children }) => {
          const owner = children.find((child) => child.name === name);
          const rec = owner?.tool === "tape" ? owner.children.find((child) => child.name === "rec") : undefined;
          if (rec?.tool === "recorder") rec.state.channels = INPUTS[index]!.channels;
        })
      } />
  );
};

/** A track's sends: a knob for each return, two to a column, in a column of the panel as its fader has one, right of its effects as a send takes the sound after them. */
const Sends = ({ page, name, sends }: { page: Page; name: string; sends: Accessor<string[]> }) => {
  const columns = memo(() => Array.from({ length: Math.ceil(sends().length / 2) }, (_, index) => index));
  return (
    <div key="sends" style={{ paddingX: "lg", paddingTop: "md", keep: true }}>
      <div style={{ height: TITLE.height, justify: "center" }}>
        <Text variant="heading">Sends</Text>
      </div>
      <div style={{ direction: "row", gap: "sm" }}>
        <For each={columns}>
          {(column) => (
            <div>
              <For each={() => sends().slice(column * 2, column * 2 + 2)}>
                {(send) => <Knob field={`${name}/${send}.gain_db`} label={returnTitle(page, send)} min={-60} max={6} default={0} unit="db" />}
              </For>
            </div>
          )}
        </For>
      </div>
    </div>
  );
};
/** Whether the sends into the return `name` take the sound before the tracks' faders. */
const PreFader = ({ page: { update, returns }, name }: { page: Page; name: string }) => {
  const on = () => {
    const back = returns().find((each) => each.name === name);
    return back?.tool === "bus" && back.state.pre_fader;
  };
  return (
    <Toggle key={`pre-fader-${name}`} size="sm" label="Pre fader" tooltip="The sends into this return take the sound before the tracks' faders" on={on}
      onClick={() =>
        update(on() ? "Send after the faders" : "Send before the faders", ({ children }) => {
          const back = children.find((child) => child.name === name);
          if (back?.tool === "bus") back.state.pre_fader = !back.state.pre_fader;
        })
      } />
  );
};

/** The master's panel: its fader and meter, and its limiter. */
export const MasterPanel = ({ page: { state, memory, update } }: { page: Page }) => (
  <div style={{ height: RACK, direction: "row", keep: true }}>
    <div style={{ width: HEADER - 1, keep: true, paddingTop: "md" }}>
      <div style={TITLE}>
        <Mark />
        <Text variant="heading" style={{ grow: true }}>Master</Text>
        <Button key="close-master" icon="x" variant="ghost" size="sm" tooltip="Close" shortcut="song.back" onClick={() => (memory.below = null)} />
      </div>
      <div style={{ direction: "row", paddingLeft: "sm" }}>
        <div style={VOLUME}>
          <Volume path="master" length={PANEL_FADER} meter="device" />
        </div>
      </div>
    </div>
    <div style={{ width: 1, background: "border", keep: true }} />
    <div style={CARDS}>
      <DeviceCard path="limit" on={() => !state.limit_off} onPower={() => update(state.limit_off ? "Turn the limiter on" : "Turn the limiter off", (song) => (song.state.limit_off = !song.state.limit_off))} />
    </div>
  </div>
);
