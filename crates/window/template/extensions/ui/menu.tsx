import { h, type IconName } from "../sdk";
import { Button, type Variant } from "./button";
import { Kbd } from "./kbd";
import { height, type Size } from "./control";
import { Tooltip } from "./tooltip";

/** A thing to do in a menu: `keys` is its key as the composer presses it, `disabled` why it cannot be done now. */
export type Item = { text: string; onPress: () => void; icon?: IconName; keys?: string; danger?: boolean; checked?: boolean; disabled?: string | boolean; line?: boolean };

/**
 * The list of a menu, inside the element that opens it (which needs a `key` and a `label`): on a
 * press, or with `"menu"` on a right press at the pointer. A press on an item closes it.
 */
export const MenuList = ({ id, label, items, open = "press", w = 220 }: { id: string; label: string; items: readonly Item[]; open?: "press" | "menu"; w?: number }) => {
  // Words line up when some items have an icon and others none.
  const icons = items.some((item) => item.icon !== undefined);
  return (
    <col overlay={{ open, side: "bottom", align: "start", gap: 4 }} role="menu" group label={label} w={w} pad={1} radius="lg" fill="raised" border="border-strong" shadow="md">
      {items.map((item) => {
        const off = item.disabled !== undefined && item.disabled !== false;
        const ink = off ? "text-muted" : item.danger ? "danger" : "text";
        return (
          <row key={`${id}-${item.text}`} label={item.text} role="menuitem" checked={item.checked} w="full" h={height("md")} px={2} gap={2} align="center" radius="md" hover={off ? undefined : "fill"} onPress={off ? undefined : item.onPress}>
            {item.checked !== undefined && <box w={14}>{item.checked && <icon name="check" size={14} color="text" />}</box>}
            {item.icon !== undefined ? <icon name={item.icon} color={item.danger ? "danger" : "text-secondary"} /> : icons && <box w={16} />}
            <text style="label" color={ink} truncate>{item.text}</text>
            {item.keys !== undefined && <row grow={1} justify="end"><Kbd keys={item.keys} /></row>}
            {typeof item.disabled === "string" && <Tooltip text={item.disabled} side="bottom" />}
          </row>
        );
      })}
    </col>
  );
};

/** A button that opens a list of things to do. */
export const Menu = ({ id, label, text, icon = "ellipsis", variant = "ghost", size = "md", items }: { id: string; label: string; text?: string; icon?: IconName; variant?: Variant; size?: Size; items: readonly Item[] }) => (
  <Button id={id} label={label} text={text} icon={text === undefined ? icon : undefined} variant={variant} size={size}>
    <MenuList id={id} label={label} items={items} w={200} />
  </Button>
);

/** The menu of the element it is in, which needs a `key` and a `label`: a right press on it, the menu key or Shift-F10 opens it at the pointer. */
export const ContextMenu = ({ id, label, w = 220, items }: { id: string; label: string; w?: number; items: readonly Item[] }) => (
  <col overlay={{ open: "menu" }} role="menu" group label={label} w={w} pad={1} radius="lg" fill="raised" border="border-strong" shadow="md">
    {rowsOf(id, items)}
  </col>
);

const rowsOf = (id: string, items: readonly Item[]) =>
  items.map((item) => {
    const look = { w: "full", h: height("md"), px: 2, gap: 2, align: "center", radius: "md" } as const;
    const parts = [
      item.icon !== undefined && <icon name={item.icon} color={item.disabled ? "text-muted" : item.danger ? "danger" : "text-secondary"} />,
      <text style="label" color={item.disabled ? "text-muted" : item.danger ? "danger" : "text"}>{item.text}</text>,
      item.keys !== undefined && <row grow={1} justify="end"><Kbd keys={item.keys} /></row>,
    ];
    const row = item.disabled ? <row {...look}>{parts}</row> : <row key={`${id}-${item.text}`} label={item.text} role="menuitem" hover="fill" onPress={item.onPress} {...look}>{parts}</row>;
    return item.line ? [<box w="full" h={1} fill="border" />, row] : row;
  });
