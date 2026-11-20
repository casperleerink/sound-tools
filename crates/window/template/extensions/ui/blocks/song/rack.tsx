import { h, type Offer, type Pointer } from "../../../sdk";
import { chainOf, chosenEffect, TAPS } from "../../../song/effects";
import { colourOf, isRow, itemAt, mixOf, rowOfKey, sendsOf, titleOf, words } from "../../../song/model";
import { HEAD, volumeOf, type Page, type Strip } from "../../../song/view";
import theme from "../../../theme.json";
import { Button } from "../../button";
import { Dot } from "../../dot";
import { control } from "../../field";
import { Fader, MAX_DB, MIN_DB } from "../../fader";
import { keysOf } from "../../kbd";
import { Knob } from "../../knob";
import { Meter } from "../../meter";
import { Picker, type PickGroup } from "../../picker";
import { Segmented } from "../../segmented";
import { Select } from "../../select";
import { Slider } from "../../slider";
import { Toggle } from "../../toggle";
import { Tooltip } from "../../tooltip";
import { TypeIn } from "../../type-in";
import { shown } from "../../value";
import { CARD_HEIGHT, Frame, HEADER } from "../device-card";
import { RegionCard } from "./region-card";

/** The track or return `name`, or `"master"`, whose panel it is; it fills the height the page gives it. */
export type RackProps = { page: Page; name: string };

const [FADER, CELL] = [112, theme.size.cell];
/** A send's row, its name, its slider and its amount, and how many rows show before they scroll. */
const [SEND, SEND_NAME, SEND_SLIDER, SEND_AMOUNT, SENDS] = [32, 56, 64, 48, 5];
/** The tools that take a sidechain: the rack offers what keys them. */
const KEYED = new Set(["compressor", "gate", "vocoder"]);
/** The channels of the input an audio track records, each alone and then the pair. */
const INPUTS = [{ text: "In 1", channels: [0] }, { text: "In 2", channels: [1] }, { text: "In 1 + 2", channels: [0, 1] }];

/**
 * The panel of a track, a return or the master: its strip at the left, then its devices in their
 * order, which scroll across, then a track's sends. An instrument's title picks another; an
 * effect turns off, goes, moves by a drag or its title's menu, and is chosen by a press, so the
 * keys move it; Add effect puts one last. An audio track shows its region chosen alone first.
 */
export const Rack = ({ page, name }: RackProps) => {
  const each = page.strips.find((strip) => strip.name === name);
  const sends = each && isRow(each) ? sendsOf(each) : [];
  return (
    <row w="full" h="full" fill="background">
      <Side page={page} name={name} each={each} />
      <box key="rack" grow={1} h="full" scroll="both">
        <row px={3} py={2} gap={2} align="start">
          {name === "master" ? <Limiter page={page} /> : each && <Devices page={page} each={each} />}
        </row>
      </box>
      {each && sends.length > 0 && <Sends page={page} each={each} sends={sends} />}
    </row>
  );
};

/** What the device of `tool` is called, as the window offers it; a plugin by the name it was offered under. */
const nameOf = (catalog: readonly Offer[], tool: string, state: unknown) => {
  const plugin = (state as { plugin_id?: unknown } | undefined)?.plugin_id;
  return catalog.find((offer) => offer.key === tool || offer.key === plugin)?.name ?? words(tool.slice(tool.lastIndexOf(".") + 1));
};

/** The devices the window offers that take `takes`, group by group, a pick each. */
const offered = (catalog: readonly Offer[], takes: Offer["takes"], here: string | null, pick: (key: string) => void): PickGroup[] => {
  const groups = new Map<string, Offer[]>();
  for (const offer of catalog.filter((each) => each.takes === takes)) groups.set(offer.group, [...(groups.get(offer.group) ?? []), offer]);
  return [...groups].map(([heading, offers]) => ({
    heading,
    picks: offers.map((offer) => ({ key: offer.key, text: offer.name, detail: offer.detail, icon: offer.icon, checked: offer.key === here, unavailable: offer.unavailable, onPick: () => pick(offer.key) })),
  }));
};

/** The instrument and the effects of a track, an audio track or a return, and Add effect. */
const Devices = ({ page, each }: { page: Page; each: Strip }) => {
  const { song, edits } = page;
  const name = each.name;
  const effects = edits.effects(name);
  const chain = chainOf(song.children, name);
  const order = chain?.state.order ?? [];
  const chosen = chosenEffect(song);
  const synth = each.tool === "track" ? each.children.find((child) => child.name === "synth") : undefined;
  /** The effect a key of this view belongs to, `null` for the instrument or Add effect: what a drag ended over goes before it. */
  const landing = (over: string | undefined): string | null | undefined => {
    if (over === "slot-instrument" || over?.startsWith("slot-instrument-")) return order[0] ?? null;
    if (over === "add-effect") return null;
    return order.find((effect) => ["", "-header", "-title", "-power", "-remove", "-sidechain"].some((end) => over === `slot-${effect}${end}`));
  };
  const dropped = (effect: string) => ({ over }: Pointer) => {
    const before = landing(over);
    if (before !== undefined) effects.moveBefore(effect, before);
  };
  const [region] = song.memory.selected;
  const clip = song.memory.selected.length === 1 && region !== undefined && rowOfKey(region) === name && itemAt(song.children, region)?.tool === "region" ? region : null;
  const empty = !synth && order.length === 0 && each.tool !== "tape";
  return (
    <row gap={2} align="start">
      {each.tool === "tape" && [clip !== null ? <RegionCard page={page} region={clip} /> : <NoClip />, <box w={1} h={CARD_HEIGHT} fill="border" />]}
      {synth && (
      <Frame id="slot-instrument" title={nameOf(song.catalog, String(synth.tool), synth.state)}
        menu={{ label: `Instrument of ${titleOf(each)}`, groups: offered(song.catalog, "notes", String(synth.tool), (key) => effects.replace(key)) }}>
        <child path={`${name}/synth`} />
      </Frame>
    )}
    {order.map((effect, at) => {
      const device = chain?.children.find((child) => child.name === effect);
      const tool = String(device?.tool ?? "");
      const title = nameOf(song.catalog, tool, device?.state);
      const moves: PickGroup = {
        picks: order.length < 2 ? [] : [
          { key: "earlier", text: "Move earlier", icon: "arrow-left", keys: shortcut(song.shortcut("effect_earlier")), unavailable: at === 0 ? "" : null, onPick: () => (effects.choose(effect), effects.step(effect, -1)) },
          { key: "later", text: "Move later", icon: "arrow-right", keys: shortcut(song.shortcut("effect_later")), unavailable: at === order.length - 1 ? "" : null, onPick: () => (effects.choose(effect), effects.step(effect, 1)) },
        ],
      };
      return (
        <Frame id={`slot-${effect}`} title={title} on={!chain?.state.off.includes(effect)} chosen={chosen?.name === effect}
          menu={{ label: `Menu of ${title}`, groups: [moves, { heading: "Replace with", picks: [] }, ...offered(song.catalog, "audio", tool, (key) => effects.replace(key, effect))] }}
          onPower={() => effects.toggle(effect)} onRemove={() => effects.remove(effect)} onChoose={() => effects.choose(effect)}
          end={KEYED.has(tool) && <Sidechain page={page} name={name} effect={effect} title={title} />}
          drag={{ x: true }} onDragEnd={dropped(effect)}>
          <child path={`${name}/chain/${effect}`} />
        </Frame>
      );
    })}
    {empty && (
      <col h={CARD_HEIGHT} px={4} justify="center">
        <text style="heading" color="text-secondary">No devices yet</text>
      </col>
    )}
    <col h={HEADER} justify="center">
      <Picker id="add-effect" label="Add effect" size="md" groups={offered(song.catalog, "audio", null, (key) => effects.add(key))}>
        <icon name="plus" size={16} color="text-secondary" />
        <text style="heading" color="text-secondary">Add effect</text>
      </Picker>
    </col>
    </row>
  );
};

const shortcut = (keys: string | null) => (keys === null ? null : keysOf(keys));

/** The first card of an audio track while none of its regions is chosen alone. */
const NoClip = () => (
  <Frame id="clip-empty" title="Clip">
    <col w={2 * CELL} h={CARD_HEIGHT - HEADER} px={4}>
      <text style="label" color="text-secondary">Select a clip.</text>
    </col>
  </Frame>
);

/** What keys an effect that takes a sidechain: Off or a track, and where on another track it takes the sound. */
const Sidechain = ({ page, name, effect, title }: { page: Page; name: string; effect: string; title: string }) => {
  const { song, edits } = page;
  const keyed = song.state.sidechains.find((each) => each.effect === `${name}/chain/${effect}`);
  const sources = [{ value: "", text: "Off" }, ...page.tracks.map((track) => ({ value: track.name, text: titleOf(track) }))];
  const trigger = (
    <row key={`slot-${effect}-sidechain`} label={`Sidechain of ${title}`} role="button" w={24} h={24} align="center" justify="center" radius="md" fill={keyed ? "fill-strong" : undefined} hover="fill">
      <icon name="audio-lines" size={14} color={keyed ? "text" : "text-secondary"} />
      <Tooltip text={keyed ? `Sidechain from ${sources.find((each) => each.value === keyed.from)?.text ?? keyed.from}` : "Sidechain"} />
      <col overlay={{ open: "press", side: "bottom", align: "end", gap: 4 }} role="dialog" label={`Sidechain of ${title}`} pad={3} gap={2} radius="lg" fill="raised" border="border-strong" shadow="md">
        <text style="heading" color="text">Sidechain</text>
        <Select id={`sidechain-${effect}`} label={`Sidechain source of ${title}`} options={sources} value={keyed?.from ?? ""} w={160} onChange={(from) => edits.effects(name).sidechain(effect, from === "" ? null : from)} />
        {keyed && keyed.from !== name && (
          <Segmented id={`tap-${effect}`} label={`Sidechain tap of ${title}`} options={TAPS.map(([value, text]) => ({ value, text }))} value={keyed.tap} onChange={(tap) => edits.effects(name).tap(effect, tap)} />
        )}
      </col>
    </row>
  );
  return keyed ? trigger : <box reveal>{trigger}</box>;
};

/** The master's limiter: power turns it off. */
const Limiter = ({ page }: { page: Page }) => {
  const off = page.song.state.limit_off;
  return (
    <Frame id="slot-limit" title="Limiter" on={!off} onPower={() => page.edits.tracks.limiter()}>
      <child path="limit" />
    </Frame>
  );
};

/** The column at the left: the mark, name and close of what the panel is of, its fader and meter, its pan, and an audio track's input or a return's Pre fader. */
const Side = ({ page, name, each }: { page: Page; name: string; each: Strip | undefined }) => {
  const { song, edits } = page;
  const title = each ? titleOf(each) : "Master";
  const pan = each ? mixOf(each)?.state.pan : undefined;
  const rec = each?.tool === "tape" ? each.children.find((child) => child.name === "rec") : undefined;
  const channels = rec?.tool === "recorder" ? rec.state.channels.join() : "";
  return (
    <col key={`strip-${name}`} w={HEAD} h="full" pad={3} gap={2} border="border" borderSides={[0, 1, 0, 0]}>
      <row w="full" gap={2} align="center">
        {each ? <Dot color={colourOf(each)} /> : <box w={8} h={8} radius="full" border="text-secondary" />}
        <text style="heading" color="text" truncate>{title}</text>
        <row grow={1} justify="end">
          <Button id="close-panel" label="Close the panel" icon="x" variant="ghost" size="sm" tip="Close" onPress={() => (song.memory.below = null)} />
        </row>
      </row>
      <row gap={2} align="start">
        <Fader {...volumeOf(page, name)} id={`fader-${name}`} label={`Volume ${title}`} height={FADER} />
        <Meter id={`meter-${name}`} label={`Level ${title}`} path={name === "master" ? "master" : `${name}/mix`} w={4} h={FADER} />
        {each && (
          <col gap={2} align="center">
            <Knob {...control(song.held, `${name}/mix.pan`, { min: -1, max: 1, unit: "pan", default: 0 }, pan ?? 0, (to) => edits.tracks.pan(name, to))} id={`pan-${name}`} label={`Pan ${title}`} text="Pan" bipolar />
            {each.tool === "tape" && (
              <Select id={`input-${name}`} label={`Input of ${title}`} options={INPUTS.map(({ text }, at) => ({ value: at, text }))} w={CELL + 8}
                value={Math.max(INPUTS.findIndex((input) => input.channels.join() === channels), 0)} onChange={(at) => edits.tracks.input(name, INPUTS[at]!.channels)} />
            )}
            {each.tool === "bus" && (
              <Toggle id={`pre-fader-${name}`} label={`Pre fader ${title}`} text="Pre fader" on={"pre_fader" in each.state && each.state.pre_fader} tip="The sends into this return take the sound before the tracks' faders" onToggle={() => edits.tracks.preFader(name)} />
            )}
          </col>
        )}
      </row>
    </col>
  );
};

/** A track's sends: a row for each return, its name, a slider and what it sends, which a click types. Five show, then they scroll. */
const Sends = ({ page, each, sends }: { page: Page; each: Strip; sends: readonly string[] }) => {
  const { song, edits } = page;
  return (
    <col key={`sends-${each.name}`} h="full" px={4} border="border" borderSides={[0, 0, 0, 1]}>
      <row h={HEADER + 8} align="center"><text style="heading" color="text">Sends</text></row>
      <col key="sends" maxH={SENDS * SEND} scroll="y">
        {sends.map((send) => {
          const back = send.slice("send-".length);
          const target = page.returns.find((one) => one.name === back);
          const title = target ? titleOf(target) : back;
          const fader = each.children.find((child) => child.name === send);
          const gain = fader?.tool === "fader" ? fader.state.gain_db : "-inf";
          const db = gain === "-inf" ? MIN_DB : gain;
          const set = (to: number) => edits.tracks.send(each.name, back, to <= MIN_DB ? "-inf" : to);
          const bound = control(song.held, `${each.name}/${send}.gain_db`, { min: MIN_DB, max: MAX_DB, unit: "db", default: 0 }, db, set);
          return (
            <row h={SEND} gap={2} align="center">
              <text style="label" color="text-secondary" w={SEND_NAME} truncate>{title}</text>
              <Slider {...bound} id={`send-${each.name}-${back}`} label={`Send to ${title}`} w={SEND_SLIDER} />
              <TypeIn id={`send-${each.name}-${back}`} label={`Send to ${title}`} value={db} unit="db" range={{ min: MIN_DB, max: MAX_DB, step: 0.1 }} w={SEND_AMOUNT} h={24} onChange={set}>
                <text style="number" color="text" w={SEND_AMOUNT} align="end">{gain === "-inf" ? "−inf" : shown(db, "db")}</text>
              </TypeIn>
            </row>
          );
        })}
      </col>
    </col>
  );
};
