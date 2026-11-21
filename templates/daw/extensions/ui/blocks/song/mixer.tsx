import { h, memo } from "../../../sdk";
import { choose } from "../../../song/clips";
import { colourOf, muted, sendsOf, titleOf } from "../../../song/model";
import { MIXER, sendOf, type Page, type Strip as Each } from "../../../song/view";
import { Button } from "../../button";
import { Dot } from "../../dot";
import { Knob } from "../../knob";
import { clearsOf } from "../../meter";
import { SHORTEST_FADER, STRIP, Strip, UNDER_FADER } from "../../strip";

export type MixerProps = { page: Page };

/** The room over and under the strips, between their parts, a strip's name, a row of sends, and the column of the returns' names left of the sends. */
const [PAD, GAP, NAME, SEND, NAMES] = [8, 4, 28, 28, 88];
/** The room inside a column's edge, the same on every side: its chosen name stands clear of it. */
const INSET = 4;
/** What a column holds over its fader: its name. */
const OVER = INSET + NAME + GAP;
/** A strip with the room around it. */
const COLUMN = STRIP + 8;

/**
 * A strip per track, which scroll, then pinned at the right behind a line the returns' and the
 * master's. A strip is its name, its channel, and under it a dial per return
 * a track sends to, lined up across the strips beside the returns' names. A click on a name
 * chooses its track, a double click shows its panel.
 */
export const Mixer = ({ page }: MixerProps) => {
  const sent = page.returns.filter((back) => page.tracks.some((each) => sendsOf(each).includes(`send-${back.name}`)));
  // The faders take the room the sends leave; with more sends than fit, the mixer scrolls.
  const fader = Math.max(MIXER - 2 * PAD - OVER - UNDER_FADER - INSET - sent.length * (GAP + SEND), SHORTEST_FADER);
  const sends = (each: Each) => sent.map((back) => [back.name, sendsOf(each).includes(`send-${back.name}`) ? (sendOf(page, each.name, back.name).value) : null] as const);
  const column = (each: Each | undefined) => {
    const name = each?.name ?? "master";
    const chosen = page.song.memory.track === name;
    const deps = [fader, chosen, each && titleOf(each), each && colourOf(each), each && muted(each), each ? sends(each) : [], each ? each.children.find((child) => child.name === "mix")?.state : page.self.children.find((child) => child.name === "master")?.state,
      page.song.state.solo.includes(name), page.song.memory.armed.includes(name), page.song.held(`${name}/mix.gain_db`), page.song.held(`${name}/mix.pan`), sent.map((back) => page.song.held(`${name}/send-${back.name}.gain_db`)).join(), clearsOf(`meter-${name}`), sent.length];
    return memo(`mixer-${name}`, deps, () => <Column page={page} each={each} sent={sent} fader={fader} />);
  };
  const tall = 2 * PAD + OVER + fader + UNDER_FADER + INSET + sent.length * (GAP + SEND);
  return (
    <col key="mixer" label="Mixer" w="full" h="full" fill="background" scroll="y">
      <row w="full" h={tall} align="start">
        {sent.length > 0 && (
          <col w={NAMES} pad={[3, 0]} gap={1}>
            <box h={PAD + OVER + fader + UNDER_FADER} />
            {sent.map((back) => <row h={SEND} align="center"><text style="label" color="text-secondary" truncate>{titleOf(back)}</text></row>)}
          </col>
        )}
        {/* A plane, which builds only the strips in view: a mixer of many tracks scrolls as one of few. */}
        <box key="mixer-tracks" label="Track strips" plane={{}} grow={1} h="full">
          {page.tracks.map((each, at) => <box place={[4 + at * (COLUMN + GAP), PAD, COLUMN, tall - 2 * PAD]}>{column(each)}</box>)}
        </box>
        {page.returns.length > 0 && (
          <row h="full" pad={[1, 2]} gap={1} border="border-strong" borderSides={[0, 0, 0, 1]}>
            {page.returns.map((each) => column(each))}
          </row>
        )}
        <row h="full" pad={[1, 2]} gap={1} border="border-strong" borderSides={[0, 0, 0, 1]}>
          {column(undefined)}
        </row>
        <row pad={2}>
          <Button id="close-mixer" label="Hide the mixer" icon="x" variant="ghost" size="sm" tip="Hide the mixer" keys={page.song.shortcut("mixer")} onPress={() => page.edits.tracks.toggleMixer()} />
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
    <col w={COLUMN} pad={1} gap={1} radius="lg" fill="background" border="border-strong">
      <row key={`strip-name-${name}`} label={`Choose ${title} strip`} role="tab" value={chosen} h={NAME} pad={[2, 0]} gap={2} align="center" radius="md" fill={chosen ? "selected" : undefined} hover="fill" onPress={(p) => press(p.clicks)}>
        <Dot color={each && (muted(each) ? "text-muted" : colourOf(each))} />
        <text style="heading" color={chosen ? "text" : "text-secondary"} truncate>{title}</text>
      </row>
      <Strip page={page} name={name} height={fader} />
      {each && each.tool !== "bus" && sent.map((back) => {
        const send = sendsOf(each).includes(`send-${back.name}`) ? sendOf(page, each.name, back.name) : undefined;
        const to = titleOf(back);
        return (
          <row h={SEND} pad={[2, 0]} align="center">
            {send && <Knob {...send} id={`send-${name}-${back.name}`} label={`${to} send of ${title}`} small={{ beside: true, w: STRIP - 36, quiet: true }} initial={-Infinity} />}
          </row>
        );
      })}
    </col>
  );
};
