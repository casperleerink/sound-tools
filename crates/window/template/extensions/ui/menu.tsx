import { h, type IconName } from "../sdk";
import { Button, type Variant } from "./button";
import { Kbd } from "./kbd";
import { height, type Size } from "./control";

/** A thing to do: `keys` do the same, `checked` marks the one of a group that holds, `divider` draws a line over it. */
export type Item = { text: string; onPress: () => void; icon?: IconName; keys?: string; danger?: boolean; checked?: boolean; divider?: true };

/**
 * The list of a menu, in what opens it: on a press of it, or `"menu"`, on a right press of it or
 * of anything in it, at the pointer.
 */
export const MenuList = ({ id, label, items, open = "press", w = 200 }: { id: string; label: string; items: readonly Item[]; open?: "press" | "menu"; w?: number }) => (
  <col overlay={open === "press" ? { open, side: "bottom", align: "start", gap: 4 } : { open }} role="menu" group label={label} w={w} pad={1} radius="lg" fill="raised" border="border-strong" shadow="md">
    {items.map((item) => [
      item.divider && <box w="full" h={1} fill="border" />,
      <row key={`${id}-${item.text}`} label={item.text} role="menuitem" checked={item.checked} w="full" h={height("md")} px={2} gap={2} align="center" radius="md" hover="fill" onPress={item.onPress}>
        {item.checked !== undefined ? <box w={16}>{item.checked && <icon name="check" size={14} color="text" />}</box> : item.icon !== undefined && <icon name={item.icon} color={item.danger ? "danger" : "text-secondary"} />}
        <text style="label" color={item.danger ? "danger" : "text"}>{item.text}</text>
        {item.keys !== undefined && <row grow={1} justify="end"><Kbd keys={item.keys} /></row>}
      </row>,
    ])}
  </col>
);

/** A button that opens a list of things to do. */
export const Menu = ({ id, label, text, icon = "ellipsis", variant = "ghost", size = "md", tip, keys, w, items }: { id: string; label: string; text?: string; icon?: IconName; variant?: Variant; size?: Size; tip?: string; keys?: string; w?: number; items: readonly Item[] }) => (
  <Button id={id} label={label} text={text} icon={text === undefined ? icon : undefined} variant={variant} size={size} tip={tip} keys={keys}>
    <MenuList id={id} label={label} items={items} w={w} />
  </Button>
);
