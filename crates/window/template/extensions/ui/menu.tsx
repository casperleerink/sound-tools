import { h, type IconName } from "../sdk";
import { Button, type Variant } from "./button";
import { Kbd } from "./kbd";
import { height, type Size } from "./control";

export type Item = { text: string; onPress: () => void; icon?: IconName; keys?: string; danger?: boolean };

/** A button that opens a list of things to do. */
export const Menu = ({ id, label, text, icon = "ellipsis", variant = "ghost", size = "md", items }: { id: string; label: string; text?: string; icon?: IconName; variant?: Variant; size?: Size; items: readonly Item[] }) => (
  <Button id={id} label={label} text={text} icon={text === undefined ? icon : undefined} variant={variant} size={size}>
    <col overlay={{ open: "press", side: "bottom", align: "start", gap: 4 }} role="menu" group label={label} w={200} pad={1} radius="lg" fill="raised" border="border-strong" shadow="md">
      {items.map((item) => (
        <row key={`${id}-${item.text}`} label={item.text} role="menuitem" w="full" h={height("md")} px={2} gap={2} align="center" radius="md" hover="fill" onPress={item.onPress}>
          {item.icon !== undefined && <icon name={item.icon} color={item.danger ? "danger" : "text-secondary"} />}
          <text style="label" color={item.danger ? "danger" : "text"}>{item.text}</text>
          {item.keys !== undefined && <row grow={1} justify="end"><Kbd keys={item.keys} /></row>}
        </row>
      ))}
    </col>
  </Button>
);
