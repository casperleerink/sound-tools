import { h, live, type Child, type Color, type Drag, type IconName, type Node, type Pointer } from "../../sdk";
import theme from "../../theme.json";
import { Picker, type PickGroup } from "../picker";
import { Tooltip } from "../tooltip";

const [ROW, PAD, ICON] = [theme.size.row, theme.space[4], theme.size.sm];
export const HEADER = theme.size.lg;
/** The body under the header: two rows of cells and the room under them. */
const BODY = 2 * ROW + theme.space[1] + PAD;
/** Every card is this tall, so a rack has a straight bottom. */
export const CARD_HEIGHT = HEADER + BODY;

/** How many columns a cell takes. */
const widthOf = (cell: Node) => ("w" in cell && typeof cell.w === "number" ? cell.w : 0);

/**
 * `cells` in two rows, read left to right: each run of cells of one width fills its top row,
 * then its bottom row, so a cell never sits over one of another width.
 */
const columns = (cells: readonly Node[]) => {
  const runs: Node[][] = [];
  for (const cell of cells) {
    const last = runs.at(-1);
    if (last && widthOf(last[0]!) === widthOf(cell)) last.push(cell);
    else runs.push([cell]);
  }
  return runs.flatMap((run) => {
    const across = Math.ceil(run.length / 2);
    return Array.from({ length: across }, (_, i) => <col gap={1}>{[run[i], run[i + across]].filter((cell) => cell !== undefined)}</col>);
  });
};

/** A page of a big device's hidden controls, picked by its tab; `changed` puts a dot on the tab, so nothing hides behind it. */
export type Section = { name: string; cells: readonly Node[]; changed?: boolean };

export type DeviceCardProps = {
  id: string;
  /** What its controls are named after for an agent, such as "More of Filter"; the frame around the card shows it. */
  title: string;
  display?: Node;
  cells?: readonly Node[];
  more?: readonly Node[];
  /** Pages of more, one shown at a time while expanded. */
  sections?: readonly Section[];
  page?: number;
  onPage?: (page: number) => void;
  expanded?: boolean;
  onExpand?: () => void;
};

/**
 * What a device shows of itself: its `display` at the left, then its `cells` in columns of two
 * rows (`Cell`, `Knob`, controls `sm`). Cards grow wider, never taller: expanded, `more` and the
 * page of `sections` show to the right of a hairline, and the edge at the right opens and closes
 * them. Its title, power and the rest are the frame's, which the rack draws around every card.
 */
export const DeviceCard = ({ id, title, display, cells = [], more = [], sections = [], page = 0, onPage, expanded = false, onExpand }: DeviceCardProps) => {
  const hides = (more.length > 0 || sections.length > 0) && onExpand !== undefined;
  const shown = sections[Math.min(Math.max(page, 0), sections.length - 1)];
  return (
    <row key={id} h={BODY} align="start">
      <row px={4} gap={2}>
        {display}
        <row>{columns(cells)}</row>
        {expanded && more.length > 0 && [<Hairline />, <row>{columns(more)}</row>]}
        {expanded && shown && [<Hairline />, sections.length > 1 && <Tabs id={id} title={title} sections={sections} page={page} onPage={onPage} />, <row>{columns(shown.cells)}</row>]}
      </row>
      {hides && [
        <col key={`${id}-more`} label={`More of ${title}`} role="switch" checked={expanded} w={20} h={2 * ROW + theme.space[1]} align="center" justify="center" radius="md" hover="fill" onPress={onExpand} actions={{ toggle: onExpand }}>
          <icon name={expanded ? "chevron-left" : "chevron-right"} size={14} color="text-secondary" />
          <Tooltip text={expanded ? "Less" : "More"} />
        </col>,
        <box w={4} />,
      ]}
    </row>
  );
};

const Hairline = () => <box w={1} h={2 * ROW} fill="border-strong" />;

/** The pages of a card's sections, one under the other: one stop of Tab, the arrows pick. */
const Tabs = ({ id, title, sections, page, onPage }: { id: string; title: string; sections: readonly Section[]; page: number; onPage?: (page: number) => void }) => (
  <col key={`${id}-pages`} label={`Pages of ${title}`} role="radiogroup" group gap={1}>
    {sections.map((section, at) => (
      <row key={`${id}-page-${at}`} label={`${title} ${section.name}`} role="radio" checked={at === page} h={ICON} px={2} gap={1} align="center" radius="md" fill={at === page ? "selected" : undefined} hover="fill" onPress={() => onPage?.(at)}>
        <text style="label" color={at === page ? "text" : "text-secondary"}>{section.name}</text>
        {section.changed && <box w={4} h={4} radius="full" fill="text-secondary" />}
      </row>
    ))}
  </col>
);

export type FrameProps = {
  /** The key of the slot: a drag of its header carries it, a press on it chooses it. */
  id: string;
  title: string;
  /** The title as a menu: another device in its place, or what to do with it. */
  menu?: { label: string; groups: readonly PickGroup[]; notes?: readonly string[] };
  /** Off, the device is passed by: its title and body are muted. */
  on?: boolean;
  onPower?: () => void;
  onRemove?: () => void;
  /** More icons of the header, before power, such as a sidechain picker. */
  end?: Child;
  chosen?: boolean;
  onChoose?: () => void;
  drag?: Drag;
  onDragEnd?: (p: Pointer) => void;
  children?: Child;
};

/**
 * The frame of a card in a rack: its surface, and a header with the title (a menu of other
 * devices), then power and remove, which show while the pointer or the keys are on it. A card's
 * own code draws none of this, so every device looks alike in a rack.
 */
export const Frame = ({ id, title, menu, on = true, onPower, onRemove, end, chosen = false, onChoose, drag, onDragEnd, children }: FrameProps) => (
  <col key={id} label={`Device ${title}`} role="group" h={CARD_HEIGHT} radius="lg" fill="surface" border={chosen ? "text-muted" : "border-strong"} hoverGroup
    {...(drag ? { opacity: live.drag(id).active.times(-0.5).plus(1) } : {})}>
    <row key={`${id}-header`} label={`Choose ${title}`} role="toolbar" group h={HEADER} px={1} gap={1} align="center"
      {...(drag ? { drag: { ...drag, id }, onDragEnd } : {})} {...(onChoose ? { onPress: onChoose } : {})}>
      {menu ? (
        <Picker id={`${id}-title`} label={menu.label} groups={menu.groups} notes={menu.notes}>
          <text style="heading" color={on ? "text" : "text-muted"} truncate>{title}</text>
          <icon name="chevron-down" size={12} color="text-muted" />
        </Picker>
      ) : (
        <row px={2}><text style="heading" color={on ? "text" : "text-muted"} truncate>{title}</text></row>
      )}
      <row grow={1} gap={1} justify="end" align="center">
        {end}
        {onPower && <Quiet shown={!on}><HeaderIcon id={`${id}-power`} label={on ? `Turn ${title} off` : `Turn ${title} on`} icon="power" color={on ? "text-secondary" : "text-muted"} checked={on} tip={on ? "Turn off" : "Turn on"} onPress={onPower} /></Quiet>}
        {onRemove && <Quiet shown={false}><HeaderIcon id={`${id}-remove`} label={`Remove ${title}`} icon="x" color="text-secondary" tip="Remove" onPress={onRemove} /></Quiet>}
      </row>
    </row>
    {/* A card taller than the rest, such as a project's own view, scrolls inside its frame. */}
    <box h={BODY} scroll="y" opacity={on ? 1 : 0.4}>{children}</box>
  </col>
);

/** Shown while `shown`, else only while the pointer or the keys are on the card. */
const Quiet = ({ shown, children }: { shown: boolean; children?: Child }) => (shown ? <box>{children}</box> : <box reveal>{children}</box>);

/** An icon of a header: 24 points to hit, a fill on hover. `checked` makes it a switch. */
export const HeaderIcon = ({ id, label, icon, color, checked, tip, onPress }: { id: string; label: string; icon: IconName; color: Color; checked?: boolean; tip: string; onPress: () => void }) => (
  <row key={id} label={label} role={checked === undefined ? "button" : "switch"} checked={checked} w={ICON} h={ICON} align="center" justify="center" radius="md" hover="fill" onPress={onPress} {...(checked === undefined ? {} : { actions: { toggle: onPress } })}>
    <icon name={icon} size={14} color={color} />
    <Tooltip text={tip} />
  </row>
);
