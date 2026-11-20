import { h, type Child, type IconName } from "../sdk";
import { height, type Size } from "./control";
import { Kbd } from "./kbd";

/** One row of a picker: `unavailable` says why it cannot be picked, and greys it. */
export type Pick = { key: string; text: string; detail?: string | null; icon?: IconName; checked?: boolean; unavailable?: string | null; keys?: string | null; onPick: () => void };
/** Rows under a quiet heading; a heading alone says what the groups under it do. */
export type PickGroup = { heading?: string; picks: readonly Pick[] };

/**
 * A button that opens a list of things to pick, in groups under headings, with a line that keeps
 * those whose name holds what is typed: devices, or what to do with one. `children` is the face
 * of the button; `notes` are quiet lines under the list.
 */
export const Picker = ({ id, label, groups, notes = [], search = true, w = 280, size = "sm", children }: { id: string; label: string; groups: readonly PickGroup[]; notes?: readonly string[]; search?: boolean; w?: number; size?: Size; children?: Child }) => (
  <row key={id} label={label} role="button" h={height(size)} px={2} gap={size === "sm" ? 1 : 2} align="center" radius="md" hover="fill">
    {children}
    <col overlay={{ open: "press", side: "bottom", align: "start", gap: 4 }} role="menu" group label={label} w={w} maxH={420} scroll="y" pad={1} radius="lg" fill="raised" border="border-strong" shadow="md">
      {search && (
        <row h={height("md")} px={2} gap={2} align="center">
          <icon name="search" size={14} color="text-muted" />
          <field key={`${id}-search`} label={`Search ${label}`} value="" placeholder="Search" focus style="label" w={w - 48} onCommit={() => {}} />
        </row>
      )}
      {groups.map((group) => [
        group.heading !== undefined && (
          <row h={height("sm")} px={2} align="end"><text style="label" color="text-muted">{group.heading}</text></row>
        ),
        group.picks.map((pick) => <Row id={id} pick={pick} />),
      ])}
      {notes.map((note) => <row px={2} py={1}><text style="label" color="text-muted">{note}</text></row>)}
    </col>
  </row>
);

const Row = ({ id, pick }: { id: string; pick: Pick }) => {
  const off = pick.unavailable !== undefined && pick.unavailable !== null;
  const detail = off ? pick.unavailable : pick.detail;
  return (
    <row key={`${id}-${pick.key}`} label={pick.text} role="menuitem" checked={pick.checked} w="full" px={2} py={1} gap={2} align="center" radius="md" hover={off ? undefined : "fill"} {...(off ? {} : { onPress: pick.onPick })}>
      {pick.icon !== undefined && <icon name={pick.icon} color={off ? "text-muted" : "text-secondary"} />}
      <col grow={1}>
        <text style="label" color={off ? "text-muted" : "text"} truncate>{pick.text}</text>
        {detail ? <text style="label" color="text-muted" truncate>{detail}</text> : null}
      </col>
      {pick.checked && <icon name="check" size={14} color="text" />}
      {pick.keys ? <Kbd keys={pick.keys} /> : null}
    </row>
  );
};
