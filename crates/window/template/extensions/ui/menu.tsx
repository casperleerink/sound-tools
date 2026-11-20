import { h, type IconName } from "../sdk";
import { Button, type Variant } from "./button";
import { Kbd } from "./kbd";
import { height, type Size } from "./control";

/** A thing to do; `disabled` says why it cannot be done here, and shows it muted. `line` draws a line over it. */
export type Item = { text: string; onPress: () => void; icon?: IconName; keys?: string; danger?: boolean; disabled?: boolean; line?: boolean };

/** A button that opens a list of things to do. */
export const Menu = ({ id, label, text, icon = "ellipsis", variant = "ghost", size = "md", items }: { id: string; label: string; text?: string; icon?: IconName; variant?: Variant; size?: Size; items: readonly Item[] }) => (
  <Button id={id} label={label} text={text} icon={text === undefined ? icon : undefined} variant={variant} size={size}>
    <col overlay={{ open: "press", side: "bottom", align: "start", gap: 4 }} role="menu" group label={label} w={200} pad={1} radius="lg" fill="raised" border="border-strong" shadow="md">
      {rowsOf(id, items)}
    </col>
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
