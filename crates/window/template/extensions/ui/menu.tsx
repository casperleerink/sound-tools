import { h } from "../sdk";
import { Button, type Variant } from "./button";
import { Kbd } from "./kbd";
import { height, type Size } from "./control";

export type Item = { text: string; onPress: () => void; keys?: string; danger?: boolean };

/** A button that opens a list of things to do. */
export const Menu = ({ id, label, text, glyph = "⋯", variant = "ghost", size = "md", items }: { id: string; label: string; text?: string; glyph?: string; variant?: Variant; size?: Size; items: readonly Item[] }) => (
  <Button id={id} label={label} text={text} glyph={text === undefined ? glyph : undefined} variant={variant} size={size}>
    <col overlay={{ open: "press", side: "bottom", align: "start", gap: 4 }} role="menu" group label={label} w={200} pad={1} radius="lg" fill="raised" border="border-strong" shadow="md">
      {items.map((item) => (
        <row key={`${id}-${item.text}`} label={item.text} role="menuitem" w="full" h={height("md")} px={2} align="center" justify="between" radius="md" hover="fill" onPress={item.onPress}>
          <text style="label" color={item.danger ? "danger" : "text"}>{item.text}</text>
          {item.keys !== undefined && <Kbd keys={item.keys} />}
        </row>
      ))}
    </col>
  </Button>
);
