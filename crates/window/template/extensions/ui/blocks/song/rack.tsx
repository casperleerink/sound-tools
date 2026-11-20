import { h } from "../../../sdk";
import { colourOf, itemAt, rowOfKey, titleOf } from "../../../song/model";
import { HEAD, volumeOf, type Page } from "../../../song/view";
import { Button } from "../../button";
import { Dot } from "../../dot";
import { Fader } from "../../fader";
import { Meter } from "../../meter";
import { RegionCard } from "./region-card";

/** The track or return `name`, or `"master"`, whose panel it is; it fills the height the page gives it. */
export type RackProps = { page: Page; name: string };

const FADER = 112;

/** The panel of a track, a return or the master: its strip, then its devices in their order, and a region chosen alone. */
export const Rack = ({ page, name }: RackProps) => {
  const each = page.strips.find((strip) => strip.name === name);
  const title = each ? titleOf(each) : "Master";
  const chain = each?.children.find((child) => child.name === "chain");
  const devices = [...(each?.children.some((child) => child.name === "synth") ? ["synth"] : []), ...(chain?.tool === "chain" ? chain.state.order.map((effect) => `chain/${effect}`) : [])];
  const [region] = page.song.memory.selected;
  const card = page.song.memory.selected.length === 1 && region !== undefined && rowOfKey(region) === name && itemAt(page.self.children, region)?.tool === "region";
  return (
    <row w="full" h="full" fill="background">
      <col key={`strip-${name}`} w={HEAD} h="full" pad={3} gap={2} border="border" borderSides={[0, 1, 0, 0]}>
        <row w="full" gap={2} align="center">
          {each ? <Dot color={colourOf(each)} /> : <box w={8} h={8} radius="full" border="text-secondary" />}
          <text style="heading" color="text" truncate>{title}</text>
          <row grow={1} justify="end">
            <Button id="close-panel" label="Close the panel" icon="x" variant="ghost" size="sm" tip="Close" onPress={() => (page.song.memory.below = null)} />
          </row>
        </row>
        <row gap={2} align="start">
          <Fader {...volumeOf(page, name)} id={`fader-${name}`} label={`Volume ${title}`} height={FADER} />
          <Meter id={`meter-${name}`} label={`Level ${title}`} path={name === "master" ? "master" : `${name}/mix`} w={4} h={FADER} />
        </row>
      </col>
      <box grow={1} h="full" scroll="both">
        <row px={3} py={3} gap={2} align="start">
          {devices.map((path) => <child path={`${name}/${path}`} />)}
          {name === "master" && <child path="limit" />}
          {card && region !== undefined && <RegionCard page={page} region={region} />}
        </row>
      </box>
    </row>
  );
};
