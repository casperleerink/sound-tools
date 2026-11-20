import { h } from "../../../sdk";
import { titleOf } from "../../../song/model";
import { volumeOf, type Page } from "../../../song/view";
import { Button } from "../../button";
import { Fader } from "../../fader";
import { Meter } from "../../meter";

export type MixerProps = { page: Page };

const FADER = 160;

/** A strip for each track and return, then the master's: it fills the height the page gives it. */
export const Mixer = ({ page }: MixerProps) => (
  <row w="full" h="full" px={3} gap={1} fill="surface" scroll="x">
    {page.strips.map((each) => <Strip page={page} name={each.name} title={titleOf(each)} />)}
    <box grow={1} />
    <Strip page={page} name="master" title="Master" />
    <Button id="close-mixer" label="Hide the mixer" icon="x" variant="ghost" size="sm" tip="Hide the mixer" onPress={() => page.edits.tracks.toggleMixer()} />
  </row>
);

const Strip = ({ page, name, title }: { page: Page; name: string; title: string }) => (
  <col key={`strip-${name}`} w={72} h="full" py={3} gap={2} align="center">
    <text style="label" color="text-secondary" truncate>{title}</text>
    <row gap={2} align="start">
      <Fader {...volumeOf(page, name)} id={`fader-${name}`} label={`Volume ${title}`} height={FADER} />
      <Meter id={`meter-${name}`} label={`Level ${title}`} path={name === "master" ? "master" : `${name}/mix`} w={4} h={FADER} />
    </row>
  </col>
);
