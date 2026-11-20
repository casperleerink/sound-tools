import { h, type IconName } from "../sdk";
import { Button, type Variant } from "./button";
import { Kbd } from "./kbd";
import { height, type Size } from "./control";
import { Tooltip } from "./tooltip";

/** A thing to do in a menu: `keys` is its key as the composer presses it, `disabled` why it cannot be done now, `line` a line over it. */
export type Item = { text: string; onPress: () => void; icon?: IconName; keys?: string; danger?: boolean; checked?: boolean; disabled?: string; line?: boolean };

/**
 * The list of a menu, inside the element that opens it (which needs a `key` and a `label`): on a
 * press, or with `"menu"` on a right press, the menu key or Shift-F10, at the pointer. A press on
 * an item closes it.
 */
export const MenuList = ({ id, label, items, open = "press", w = 220 }: { id: string; label: string; items: readonly Item[]; open?: "press" | "menu"; w?: number }) => {
  // Words line up when some items have an icon and others none.
  const icons = items.some((item) => item.icon !== undefined);
  return (
    <col overlay={{ open, side: "bottom", align: "start", gap: 4 }} role="menu" group label={label} w={w} pad={1} radius="lg" fill="raised" border="border-strong" shadow="md">
      {items.map((item) => {
        const why = item.disabled;
        const off = why !== undefined;
        const row = (
          <row key={`${id}-${item.text}`} label={item.text} role="menuitem" checked={item.checked} w="full" h={height("md")} px={2} gap={2} align="center" radius="md" hover={off ? undefined : "fill"} onPress={off ? undefined : item.onPress}>
            {item.checked !== undefined && <box w={14}>{item.checked && <icon name="check" size={14} color="text" />}</box>}
            {item.icon !== undefined ? <icon name={item.icon} color={off ? "text-muted" : item.danger ? "danger" : "text-secondary"} /> : icons && <box w={16} />}
            <text style="label" color={off ? "text-muted" : item.danger ? "danger" : "text"} truncate>{item.text}</text>
            {item.keys !== undefined && <row grow={1} justify="end"><Kbd keys={item.keys} /></row>}
            {why !== undefined && <Tooltip text={why} side="bottom" />}
          </row>
        );
        return item.line ? [<box w="full" h={1} fill="border" />, row] : row;
      })}
    </col>
  );
};

/** A button that opens a list of things to do. */
export const Menu = ({ id, label, text, icon = "ellipsis", variant = "ghost", size = "md", tip, keys, w = 200, items }: { id: string; label: string; text?: string; icon?: IconName; variant?: Variant; size?: Size; tip?: string; keys?: string; w?: number; items: readonly Item[] }) => (
  <Button id={id} label={label} text={text} icon={text === undefined ? icon : undefined} variant={variant} size={size} tip={tip} keys={keys}>
    <MenuList id={id} label={label} items={items} w={w} />
  </Button>
);
