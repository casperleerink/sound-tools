import { h, type Offer, type Pointer } from "../../../sdk";
import { chainOf, chosenEffect, TAPS } from "../../../song/effects";
import { colourOf, isRow, itemAt, rowOfKey, sendsOf, titleOf } from "../../../song/model";
import { deviceName, HEAD, sendOf, type Page, type Strip as Channel } from "../../../song/view";
import theme from "../../../theme.json";
import { Button } from "../../button";
import { Dot } from "../../dot";
import { Knob } from "../../knob";
import { MenuList, type Group, type Item } from "../../menu";
import { Segmented } from "../../segmented";
import { Select } from "../../select";
import { SHORTEST_FADER, Strip } from "../../strip";
import { Toggle } from "../../toggle";
import { Tooltip } from "../../tooltip";
import { CARD_HEIGHT, Frame, HEADER } from "../device-card";
import { RegionCard } from "./region-card";

/** The track or return `name`, or `"master"`, whose panel it is; it fills the height the page gives it. */
export type RackProps = { page: Page; name: string };

const [FADER, CELL] = [SHORTEST_FADER, theme.size.cell];
/** A send's row, its name and its amount, and how many rows show before they scroll. */
const [SEND, SEND_NAME, SEND_AMOUNT, SENDS] = [32, 64, 52, 5];
/** The tools that take a sidechain: the rack offers what keys them. */
const KEYED = new Set(["compressor", "gate", "vocoder"]);
/** The channels of the input an audio track records, each alone and then the pair. */
const INPUTS = [{ text: "In 1", channels: [0] }, { text: "In 2", channels: [1] }, { text: "Both", channels: [0, 1] }];

/**
 * The panel of a track, a return or the master: its strip at the left, then its devices in their
 * order, which scroll across, then a track's sends. An instrument's title picks another; an
 * effect turns off, goes, moves by a drag or its title's menu, and is chosen by a press, so the
 * keys move it; Add effect puts one last. An audio track shows its region chosen alone first.
 * Its close is at the right, as the mixer's and the note editor's.
 */
export const Rack = ({ page, name }: RackProps) => {
  const each = page.strips.find((strip) => strip.name === name);
  const sends = each && isRow(each) ? sendsOf(each) : [];
  return (
    <row w="full" h="full" fill="background">
      <Side page={page} name={name} each={each} />
      <box key="rack" grow={1} h="full" scroll="x">
        <row pad={[3, 2]} gap={2} align="start">
          {name === "master" ? <Limiter page={page} /> : each && <Devices page={page} each={each} />}
        </row>
      </box>
      {each && sends.length > 0 && <Sends page={page} each={each} sends={sends} />}
      <col h="full" pad={[2, 2]}>
        <row h={HEADER} align="center">
          <Button id="close-panel" label="Close the panel" icon="x" variant="ghost" size="sm" tip="Close" onPress={() => (page.song.memory.below = null)} />
        </row>
      </col>
    </row>
  );
};

/** The devices the window offers that take `takes`, group by group, a pick each. */
const offered = (catalog: readonly Offer[], takes: Offer["takes"], here: string | null, pick: (key: string) => void): Group[] => {
  const groups = new Map<string, Offer[]>();
  for (const offer of catalog.filter((each) => each.takes === takes)) groups.set(offer.group, [...(groups.get(offer.group) ?? []), offer]);
  return [...groups].map(([heading, offers]) => ({
    heading,
    items: offers.map((offer) => ({ key: offer.key, text: offer.name, detail: offer.detail, icon: offer.icon, ...(here === null ? {} : { checked: offer.key === here }), disabled: offer.unavailable, onPress: () => pick(offer.key) })),
  }));
};

/** The instrument and the effects of a track, an audio track or a return, and Add effect. */
const Devices = ({ page, each }: { page: Page; each: Channel }) => {
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
        <Frame id="slot-instrument" title={deviceName(song.catalog, synth)}
          menu={{ label: `Instrument of ${titleOf(each)}`, items: offered(song.catalog, "notes", String(synth.tool), (key) => effects.replace(key)) }}>
          <child path={`${name}/synth`} />
        </Frame>
      )}
      {order.map((effect, at) => {
        const device = chain?.children.find((child) => child.name === effect);
        const tool = String(device?.tool ?? "");
        const title = deviceName(song.catalog, { tool, state: device?.state });
        const moves: Item[] = order.length < 2 ? [] : [
          { key: "earlier", text: "Move earlier", icon: "arrow-left", keys: song.shortcut("effect_earlier"), disabled: at === 0 ? "" : null, onPress: () => (effects.choose(effect), effects.step(effect, -1)) },
          { key: "later", text: "Move later", icon: "arrow-right", keys: song.shortcut("effect_later"), disabled: at === order.length - 1 ? "" : null, onPress: () => (effects.choose(effect), effects.step(effect, 1)) },
        ];
        return (
          <Frame id={`slot-${effect}`} title={title} on={!chain?.state.off.includes(effect)} chosen={chosen?.name === effect}
            menu={{ label: `Menu of ${title}`, items: [...moves, { heading: "Replace with", items: [] }, ...offered(song.catalog, "audio", tool, (key) => effects.replace(key, effect))] }}
            onPower={() => effects.toggle(effect)} onRemove={() => effects.remove(effect)} onChoose={() => effects.choose(effect)}
            end={KEYED.has(tool) && <Sidechain page={page} name={name} effect={effect} title={title} />}
            drag={{ x: true }} onDragEnd={dropped(effect)}>
            <child path={`${name}/chain/${effect}`} />
          </Frame>
        );
      })}
      {empty && (
        <col h={HEADER} pad={[2, 0]} justify="center">
          <text style="heading" color="text-muted">No devices yet</text>
        </col>
      )}
      <col h={HEADER} justify="center">
        <Button id="add-effect" label="Add effect" text="Add effect" icon="plus" variant="ghost" size="sm">
          <MenuList id="add-effect" label="Add effect" items={offered(song.catalog, "audio", null, (key) => effects.add(key))} w={280} search />
        </Button>
      </col>
    </row>
  );
};

/** The first card of an audio track while none of its regions is chosen alone. */
const NoClip = () => (
  <Frame id="clip-empty" title="Clip">
    <col w={2 * CELL} h={CARD_HEIGHT - HEADER} pad={[4, 0]}>
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

/** The column at the left: the mark and name of what the panel is of, its strip, and an audio track's input or a return's Pre fader. */
const Side = ({ page, name, each }: { page: Page; name: string; each: Channel | undefined }) => {
  const { edits } = page;
  const title = each ? titleOf(each) : "Master";
  const rec = each?.tool === "tape" ? each.children.find((child) => child.name === "rec") : undefined;
  const channels = rec?.tool === "recorder" ? rec.state.channels.join() : "";
  return (
    <col key={`side-${name}`} w={HEAD} h="full" pad={[3, 2]} gap={1} border="border" borderSides={[0, 1, 0, 0]}>
      <row w="full" h={HEADER} gap={2} align="center">
        <Dot color={each && colourOf(each)} />
        <text style="heading" color="text" truncate>{title}</text>
      </row>
      <Strip page={page} name={name} height={FADER} full>
        {each?.tool === "tape" && (
          <Select id={`input-${name}`} label={`Input of ${title}`} options={INPUTS.map(({ text }, at) => ({ value: at, text }))} w={CELL}
            value={Math.max(INPUTS.findIndex((input) => input.channels.join() === channels), 0)} onChange={(at) => edits.tracks.input(name, INPUTS[at]!.channels)} />
        )}
        {each?.tool === "bus" && (
          <Toggle id={`pre-fader-${name}`} label={`Pre fader ${title}`} text="Pre" on={"pre_fader" in each.state && each.state.pre_fader} tip="The sends into this return take the sound before the tracks' faders" onToggle={() => edits.tracks.preFader(name)} />
        )}
      </Strip>
    </col>
  );
};

/** A track's sends: a row for each return, a small dial, its name and what it sends, which a click types. Five show, then they scroll. */
const Sends = ({ page, each, sends }: { page: Page; each: Channel; sends: readonly string[] }) => (
  <col key={`sends-${each.name}`} h="full" pad={[4, 2]} border="border" borderSides={[0, 0, 0, 1]}>
    <row h={HEADER} align="center"><text style="heading" color="text">Sends</text></row>
    <col key="sends" maxH={SENDS * SEND} scroll="y">
      {sends.map((send) => {
        const back = send.slice("send-".length);
        const target = page.returns.find((one) => one.name === back);
        const title = target ? titleOf(target) : back;
        return (
          <row h={SEND} gap={2} align="center">
            <Knob {...sendOf(page, each.name, back)} id={`send-${each.name}-${back}`} label={`Send to ${title}`} small={{ w: SEND_AMOUNT, beside: true }} />
            <text style="label" color="text-secondary" w={SEND_NAME} truncate>{title}</text>
          </row>
        );
      })}
    </col>
  </col>
);
