import { h, type IconName } from "../sdk";
import { Button, type Variant } from "./button";
import { Kbd } from "./kbd";
import { height, type Size } from "./control";

/**
 * A thing to do in a menu: `keys` is its key as the composer presses it, `detail` a quiet line
 * under it, `disabled` why it cannot be done now (said in that line), `line` a line over it.
 * `key` and `label` default to `text`.
 */
export type Item = { text: string; onPress: () => void; key?: string; label?: string; icon?: IconName; keys?: string; detail?: string | null; danger?: boolean; checked?: boolean; disabled?: string | null; line?: boolean };
/** Items under a quiet heading; a heading alone says what the groups under it do. */
export type Group = { heading: string; items: readonly Item[] };

/** The line at the top of a list that keeps the items whose name holds what is typed. */
export const Search = ({ id, label, w }: { id: string; label: string; w: number }) => (
  <row h={height("md")} pad={[2, 0]} gap={2} align="center">
    <icon name="search" size={14} color="text-muted" />
    <field key={`${id}-search`} label={`Search ${label}`} value="" placeholder="Search" focus style="label" w={w - 48} onCommit={() => {}} />
  </row>
);

const Row = ({ id, item, icons }: { id: string; item: Item; icons: boolean }) => {
  const off = item.disabled !== undefined && item.disabled !== null;
  const detail = off ? item.disabled : item.detail;
  const ink = off ? "text-muted" : item.danger ? "danger" : "text";
  return (
    <row key={`${id}-${item.key ?? item.text}`} label={item.label ?? item.text} role="menuitem" value={item.checked} w="full" {...(detail ? {} : { h: height("md") })} pad={[2, detail ? 1 : 0]} gap={2} align="center" radius="md" hover={off ? undefined : "fill"} onPress={off ? undefined : item.onPress}>
      {item.checked !== undefined && <box w={14}>{item.checked && <icon name="check" size={14} color="text" />}</box>}
      {item.icon !== undefined ? <icon name={item.icon} color={off ? "text-muted" : item.danger ? "danger" : "text-secondary"} /> : icons && <box w={16} />}
      <col grow={1}>
        <text style="label" color={ink} truncate>{item.text}</text>
        {detail ? <text style="label" color="text-muted" truncate>{detail}</text> : null}
      </col>
      {item.keys !== undefined && <Kbd keys={item.keys} />}
    </row>
  );
};

/**
 * The list of a menu, inside the element that opens it (which needs a `key` and a `label`): on a
 * press, or with `"menu"` on a right press, the menu key or Shift-F10, at the pointer. A press on
 * an item closes it. `search` adds a line that keeps the items it names, and the groups that hold one.
 */
export const MenuList = ({ id, label, items, open = "press", w = 220, search = false, notes = [] }: { id: string; label: string; items: readonly (Item | Group)[]; open?: "press" | "menu"; w?: number; search?: boolean; notes?: readonly string[] }) => {
  const all = items.flatMap((each) => ("heading" in each ? each.items : [each]));
  // Words line up when some items have an icon and others none.
  const icons = all.some((item) => item.icon !== undefined);
  const row = (item: Item) => {
    const shown = <Row id={id} item={item} icons={icons} />;
    return item.line ? [<box w="full" h={1} fill="border" />, shown] : shown;
  };
  return (
    <col overlay={{ open, side: "bottom", align: "start", gap: 4 }} role="menu" group label={label} w={w} maxH={420} scroll="y" pad={1} radius="lg" fill="raised" border="border-strong" shadow="md">
      {search && <Search id={id} label={label} w={w} />}
      {items.map((each) => ("heading" in each
        ? (
          <col key={`${id}-group-${each.heading}`} w="full">
            <row h={height("sm")} pad={[2, 0]} align="end"><text style="label" color="text-muted">{each.heading}</text></row>
            {each.items.map(row)}
          </col>
        )
        : row(each)))}
      {notes.map((note) => <row pad={[2, 1]}><text style="label" color="text-muted">{note}</text></row>)}
    </col>
  );
};

/** A button that opens a list of things to do. */
export const Menu = ({ id, label, text, icon = "ellipsis", variant = "ghost", size = "md", tip, keys, w = 200, items }: { id: string; label: string; text?: string; icon?: IconName; variant?: Variant; size?: Size; tip?: string; keys?: string; w?: number; items: readonly (Item | Group)[] }) => (
  <Button id={id} label={label} text={text} icon={text === undefined ? icon : undefined} variant={variant} size={size} tip={tip} keys={keys}>
    <MenuList id={id} label={label} items={items} w={w} />
  </Button>
);
