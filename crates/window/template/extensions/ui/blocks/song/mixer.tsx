import { h, memo } from "../../../sdk";
import { choose } from "../../../song/clips";
import { colourOf, muted, sendsOf, titleOf } from "../../../song/model";
import { MIXER, sendOf, type Page, type Strip as Each } from "../../../song/view";
import { Button } from "../../button";
import { Dot } from "../../dot";
import { Knob } from "../../knob";
import { clearsOf } from "../../meter";
import { SHORTEST_FADER, STRIP, Strip, VALUE_LINE } from "../../strip";

export type MixerProps = { page: Page };

/** The room over and under the strips, between their parts, a strip's name, a row of sends, and the column of the returns' names left of the sends. */
const [PAD, GAP, NAME, SEND, NAMES] = [8, 4, 28, 28, 64];
/** What a column holds over its fader: its name; and under it, its value line. */
const OVER = NAME + GAP;
const UNDER = VALUE_LINE;

/**
 * A strip per track, then behind a line the returns' and the master's, which stay in view while
 * the tracks' scroll under them. A strip is its name, its channel, and under it a dial per return
 * a track sends to, lined up across the strips beside the returns' names. A click on a name
 * chooses its track, a double click shows its panel.
 */
export const Mixer = ({ page }: MixerProps) => {
  const sent = page.returns.filter((back) => page.tracks.some((each) => sendsOf(each).includes(`send-${back.name}`)));
  // The faders take the room the sends leave; with more sends than fit, the mixer scrolls.
  const fader = Math.max(MIXER - 2 * PAD - OVER - UNDER - sent.length * (GAP + SEND), SHORTEST_FADER);
  const sends = (each: Each) => sent.map((back) => [back.name, sendsOf(each).includes(`send-${back.name}`) ? (sendOf(page, each.name, back.name).value) : null] as const);
  const column = (each: Each | undefined) => {
    const name = each?.name ?? "master";
    const chosen = page.song.memory.track === name;
    const deps = [fader, chosen, each && titleOf(each), each && colourOf(each), each && muted(each), JSON.stringify(each ? sends(each) : []), JSON.stringify(each ? each.children.find((child) => child.name === "mix")?.state : page.self.children.find((child) => child.name === "master")?.state),
      page.song.state.solo.includes(name), page.song.memory.armed.includes(name), page.song.held(`${name}/mix.gain_db`), page.song.held(`${name}/mix.pan`), sent.map((back) => page.song.held(`${name}/send-${back.name}.gain_db`)).join(), clearsOf(`meter-${name}`), sent.length];
    return memo(`mixer-${name}`, deps, () => <Column page={page} each={each} sent={sent} fader={fader} />);
  };
  const key = page.song.shortcut("mixer");
  return (
    <col key="mixer" label="Mixer" w="full" h="full" fill="surface" scroll="y">
      <row w="full" h={2 * PAD + OVER + fader + UNDER + sent.length * (GAP + SEND)} align="start">
        {sent.length > 0 && (
          <col w={NAMES} px={3} gap={1}>
            <box h={PAD + OVER + fader + UNDER} />
            {sent.map((back) => <row h={SEND} align="center"><text style="label" color="text-secondary" truncate>{titleOf(back)}</text></row>)}
          </col>
        )}
        <box key="mixer-tracks" label="Track strips" scroll="x" h="full">
          <row px={1} py={2} gap={1}>
            {page.tracks.map((each) => column(each))}
          </row>
        </box>
        {page.returns.length > 0 && (
          <row h="full" px={1} py={2} gap={1} border="border-strong" borderSides={[0, 0, 0, 1]}>
            {page.returns.map((each) => column(each))}
          </row>
        )}
        <row h="full" px={1} py={2} gap={1} border="border-strong" borderSides={[0, 0, 0, 1]}>
          {column(undefined)}
        </row>
        <row grow={1} justify="end" pad={2}>
          <Button id="close-mixer" label="Hide the mixer" icon="x" variant="ghost" size="sm" tip={key ? `Hide the mixer (${key.toUpperCase()})` : "Hide the mixer"} onPress={() => page.edits.tracks.toggleMixer()} />
        </row>
      </row>
    </col>
  );
};

/** A strip under its name, and its sends: of a track, a return, or the master while `each` is none. */
const Column = ({ page, each, sent, fader }: { page: Page; each: Each | undefined; sent: readonly Each[]; fader: number }) => {
  const name = each?.name ?? "master";
  const title = each ? titleOf(each) : "Master";
  const chosen = page.song.memory.track === name;
  const press = (clicks: number) => (each ? (clicks === 2 ? page.edits.tracks.openPanel(each.name) : choose(page.song, [], each.name)) : clicks === 2 && page.edits.tracks.openMaster());
  return (
    <col w={STRIP + 4} px={1} gap={1} radius="lg" fill={chosen ? "selected" : "fill/50"}>
      <row key={`strip-name-${name}`} label={`Choose ${title} strip`} role="tab" checked={chosen} h={NAME} px={2} gap={2} align="center" radius="md" hover="fill/50" onPress={(p) => press(p.clicks)}>
        {each ? <Dot color={muted(each) ? "text-muted" : colourOf(each)} /> : <box w={8} h={8} radius="full" border="text-secondary" />}
        <text style="label" color={chosen ? "text" : "text-secondary"} truncate>{title}</text>
      </row>
      <Strip page={page} name={name} height={fader} />
      {each && each.tool !== "bus" && sent.map((back) => {
        const send = sendsOf(each).includes(`send-${back.name}`) ? sendOf(page, each.name, back.name) : undefined;
        const to = titleOf(back);
        return (
          <row h={SEND} px={2} align="center">
            {send && <Knob {...send} id={`send-${name}-${back.name}`} label={`${to} send of ${title}`} small={{ beside: true, w: STRIP - 36, quiet: true }} initial={-Infinity} />}
          </row>
        );
      })}
    </col>
  );
};
