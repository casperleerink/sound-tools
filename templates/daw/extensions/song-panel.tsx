import { Button, DeviceCard, For, Fragment, h, Knob, Select, Show, Text, Toggle, type Accessor, type CardSelect, type MenuItem } from "./sdk";
import { Gain, returnTitle, sendsOf, Volume } from "./song-mixer";
import { colourOf, HEADER, inputOf, itemTool, Mark, muted, nameFrom, titleOf, type Page } from "./song-page";
import type { Song } from "./song";
import { freeName } from "./track";

// What shows under the timeline for one track or return, its panel, and for the master.

// The panel of the chosen track, as tall as a card and the room around it.
/** A device card's height, and the panel's: its cards, 12 points above them and under. */
const CARD = 192;
const RACK = CARD + 24;
// The fader of the chosen track's panel.
const PANEL_FADER = 112;
/** A send's row, how wide the return's name and what it sends are beside its dial, and how many rows show before they scroll. */
const [SEND, SEND_NAME, SEND_AMOUNT, SENDS] = [32, 48, 44, 5];
/** The top of a panel's first column: its mark, its name and its close button. */
const TITLE = { height: 32, direction: "row", align: "center", gap: "sm", paddingLeft: "lg", paddingRight: "sm" } as const;
/** A fader over its value, under the title. */
const VOLUME = { align: "center", gap: "sm", paddingTop: "xs" } as const;
/** The cards of a panel, side by side, each in a `SLOT`: 12 points apart, and 12 above them. */
const CARDS = { grow: true, direction: "row", gap: "xs", paddingX: "md", paddingTop: "sm", align: "start" } as const;
/** What a card of a panel is in: the room around it where the ring of its tab stop shows, and the fill of the chosen one, which a card that filled it would cover. */
const SLOT = { padding: "xs", radius: "lg", keep: true } as const;
/** A surface as tall as a device card and with its look, for what a panel shows beside them. */
const SURFACE = { height: CARD, radius: "lg", background: "surface", borderColor: "border", borderWidth: 1, paddingX: "lg", keep: true } as const;
/** Where on a track a sidechain takes its sound, and what its select says, as the arrangement's. */
const TAPS = [["pre_fx", "Pre FX"], ["post_fx", "Post FX"], ["post_fader", "Post mixer"]] as const;
type Tap = (typeof TAPS)[number][0];

/** The chain of the track or return `name`. */
const chainOf = (children: Song["children"], name: string) => {
  const row = children.find((each) => each.name === name);
  const effects = row?.tool === "track" || row?.tool === "tape" || row?.tool === "bus" ? row.children.find((child) => child.name === "chain") : undefined;
  return effects?.tool === "chain" ? effects : undefined;
};
/** Changes the chain of the track or return `name` as one undo step `label`. */
const editChain = ({ update }: Pick<Song, "update">, name: string, label: string, change: (effects: { state: { order: string[]; off: string[] }; children: { name: string }[] }) => void) =>
  update(label, ({ children }) => {
    const row = children.find((child) => child.name === name);
    const effects = row?.tool === "track" || row?.tool === "tape" || row?.tool === "bus" ? row.children.find((child) => child.name === "chain") : undefined;
    if (effects?.tool === "chain") change(effects);
  });
/** Puts the effect `moved` of `name` before `before` in its chain, or last, as one undo step. */
function moveEffect(tool: Pick<Song, "children" | "update">, name: string, moved: string, before: string | null) {
  if (!chainOf(tool.children, name)?.state.order.includes(moved) || moved === before) return;
  editChain(tool, name, "Move an effect", (effects) => {
    const rest = effects.state.order.filter((each) => each !== moved);
    rest.splice(before === null ? rest.length : rest.indexOf(before), 0, moved);
    effects.state.order = rest;
  });
}
/** Moves the effect `effect` of `name` a step earlier or later in its chain; at its end it stays. */
export function stepEffect(tool: Pick<Song, "children" | "update">, name: string, effect: string, by: -1 | 1) {
  const order = chainOf(tool.children, name)?.state.order ?? [];
  const at = order.indexOf(effect);
  const other = order[at + by];
  if (at >= 0 && other !== undefined) moveEffect(tool, name, effect, by < 0 ? other : (order[at + 2] ?? null));
}
/** The chosen effect, while the panel of its track shows and its chain holds it: the one the keys move. */
export function chosenEffect({ memory, children }: Pick<Song, "memory" | "children">) {
  const chosen = memory.effect;
  const shown = chosen !== null && memory.below?.kind === "track" && memory.below.name === chosen.track;
  return shown && chainOf(children, chosen.track)?.state.order.includes(chosen.name) ? chosen : null;
}

/**
 * A device the composer picks from what the window offers that takes `takes`, as the window's
 * pickers list them, under `heading`, which says what a pick does; or one of `first`, the
 * items over them, by its place.
 */
async function pick({ offers, openMenu }: Page, takes: "notes" | "audio", at?: string, first: readonly MenuItem[] = [], heading?: string) {
  const { offers: offered, notes, here } = await offers(takes, { at });
  // A menu has one level of headings: `heading` is a line of the first group's, over its own.
  const headed = (group: string) => (heading !== undefined && group === offered[0]?.group ? `${heading}\n${group}` : group);
  const items = offered.map(({ key, name, detail, group, icon, unavailable }) => ({ label: name, detail: detail ?? undefined, group: headed(group), icon, checked: key === here, disabled: unavailable !== null }));
  const picked = await openMenu([...first, ...items], { notes, width: 280, maxHeight: 540, search: true });
  return picked === null ? undefined : picked < first.length ? picked : offered[picked - first.length];
}

/**
 * The devices of the track or return `name`: an instrument, which can be changed, or the card
 * of an audio track's chosen region, then the effects of its chain, which can be added, turned
 * off, removed and put in another order: by a drag, from the menu of its title, or, the chosen
 * one, by the keys of the song's actions. An action cannot see where the focus is, so the song
 * keeps which effect is chosen: enter on its card, the menu of its title or adding it chooses
 * it, with the fill of what is chosen around it. They scroll on their own: a track's sends stay
 * at the right, however long its chain is.
 */
export const Rack = ({ page, name }: { page: Page; name: string }) => {
  const { state, children, memory, update, addOffer, rows, strips, toggleMute, toggleSolo } = page;
  const chain = () => chainOf(children, name);
  const order = () => chain()?.state.order ?? [];
  const isOff = (effect: string) => chain()?.state.off.includes(effect) ?? false;
  const choose = (effect: string) => (memory.effect = { track: name, name: effect });
  const addEffect = async () => {
    const offer = await pick(page, "audio");
    if (typeof offer !== "object") return;
    const taken = chain()?.children.map((child) => child.name) ?? [];
    const effect = freeName(taken, nameFrom(offer.key) || "effect");
    addOffer(offer.key, `${name}/chain/${effect}`, "Add an effect");
    editChain(page, name, "Add an effect", (effects) => (effects.state.order = [...effects.state.order, effect]));
    choose(effect);
  };
  /**
   * The menu of a card's title: another device in place of the one at `path`, an instrument
   * from what takes notes, an effect from what takes sound, which a heading says; and over
   * them, for an `effect` of a chain of several, a step earlier or later in it, with the keys
   * that do it. Its menu is about that effect, so it chooses it.
   */
  const replace = async (takes: "notes" | "audio", path: string, label: string, effect?: string) => {
    if (effect !== undefined) choose(effect);
    const at = effect === undefined ? -1 : order().indexOf(effect);
    const moves: MenuItem[] = effect === undefined || order().length < 2 ? [] : [
      { label: "Move earlier", icon: "arrow-left", shortcut: "song.effect_earlier", disabled: at === 0 },
      { label: "Move later", icon: "arrow-right", shortcut: "song.effect_later", disabled: at === order().length - 1 },
    ];
    const picked = await pick(page, takes, path, moves, effect === undefined ? undefined : "Replace with");
    if (picked === undefined) return;
    if (typeof picked === "number") {
      if (effect !== undefined) stepEffect(page, name, effect, picked === 0 ? -1 : 1);
      return;
    }
    unkey(path, label);
    addOffer(picked.key, path, label);
  };
  /** Takes the sidechain off the effect at `path`, as part of the step `label`: it goes, or another device takes its place. */
  const unkey = (path: string, label: string) => {
    if (state.sidechains.some((each) => each.effect === path)) update(label, (song) => (song.state.sidechains = song.state.sidechains.filter((each) => each.effect !== path)));
  };
  const remove = (effect: string) => {
    unkey(`${name}/chain/${effect}`, "Remove an effect");
    editChain(page, name, "Remove an effect", (effects) => {
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
    editChain(page, name, isOff(effect) ? "Turn an effect on" : "Turn an effect off", (effects) => {
      effects.state.off = effects.state.off.includes(effect) ? effects.state.off.filter((each) => each !== effect) : [...effects.state.off, effect];
    });
  // An effect dragged onto another goes before it, onto the instrument first, onto Add effect last; its title's menu moves it a step.
  const moveBefore = (moved: string, before: string | null) => moveEffect(page, name, moved, before);
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
              <div style={SLOT}>
                {() => {
                  const chosen = [...memory.selected].find((key) => key.startsWith(`${name}/`) && itemTool(children, key) === "region");
                  return chosen ? <DeviceCard key={`clip-${chosen}`} path={chosen} /> : <EmptyClip />;
                }}
              </div>
              <div style={SLOT}>
                <div style={{ width: 1, height: CARD, background: "border", keep: true }} />
              </div>
            </>
          ) : row()?.tool === "track" ? (
            <div key="instrument" style={SLOT} onDrop={({ carried }) => carried && order()[0] !== undefined && moveBefore(carried, order()[0]!)}>
              <DeviceCard path={`${name}/synth`} onTitle={() => replace("notes", `${name}/synth`, "Change the instrument")} />
            </div>
          ) : null
        }
        <For each={order}>
          {(effect) => (
            <div key={`effect-${effect}`} carry={effect} onDrop={({ carried }) => carried && moveBefore(carried, effect)}
              tabStop keys={["enter"]} onKey={() => choose(effect)} style={() => ({ ...SLOT, background: chosenEffect(page)?.name === effect ? "selected" : undefined })}>
              <DeviceCard path={`${name}/chain/${effect}`} on={() => !isOff(effect)} onPower={() => toggle(effect)} onClose={() => remove(effect)}
                onTitle={() => replace("audio", `${name}/chain/${effect}`, "Change an effect", effect)}
                sidechain={() => sidechainOf(effect)} />
            </div>
          )}
        </For>
        <div key="add-effect" style={SLOT} onDrop={({ carried }) => carried && moveBefore(carried, null)}>
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

/**
 * A track's sends: a row for each return, its dial, the return's name and what it sends, which
 * a double click or enter types, one under the other in a column of the panel as its fader has
 * one, right of its effects as a send takes the sound after them. Five rows fit, and it ends as far over the
 * window's edge as a card does; more scroll down, and the cut one fades.
 */
const Sends = ({ page, name, sends }: { page: Page; name: string; sends: Accessor<string[]> }) => (
  <div style={{ paddingX: "lg", paddingTop: "md", keep: true }}>
    <div style={{ height: TITLE.height, justify: "center" }}>
      <Text variant="heading">Sends</Text>
    </div>
    <div key="sends" style={{ height: SENDS * SEND, scroll: "y" }}>
      <For each={sends}>
        {(send) => {
          const title = returnTitle(page, send);
          return (
            <div style={{ height: SEND, direction: "row", align: "center", gap: "sm", keep: true }}>
              <Knob field={`${name}/${send}.gain_db`} label={title} min={-60} max={6} default={0} unit="db" text="none" />
              <Text variant="label" style={{ width: SEND_NAME, truncate: true, keep: true }}>{title}</Text>
              <Gain path={`${name}/${send}`} width={SEND_AMOUNT} align="right" label={`Change ${title.toLowerCase()}`} />
            </div>
          );
        }}
      </For>
    </div>
  </div>
);
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
      <div style={SLOT}>
        <DeviceCard path="limit" on={() => !state.limit_off} onPower={() => update(state.limit_off ? "Turn the limiter on" : "Turn the limiter off", (song) => (song.state.limit_off = !song.state.limit_off))} />
      </div>
    </div>
  </div>
);
