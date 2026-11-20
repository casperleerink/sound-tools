import { h, type Child, type IconName } from "../sdk";
import { Button, type Variant } from "./button";
import type { Size } from "./control";

/** A button that opens a panel of more controls beside it; a press outside or Escape closes it. */
export const Popover = ({ id, label, text, icon, title, variant = "subtle", size = "md", children }: { id: string; label: string; text?: string; icon?: IconName; title?: string; variant?: Variant; size?: Size; children?: Child }) => (
  <Button id={id} label={label} text={text} icon={icon} variant={variant} size={size}>
    <col overlay={{ open: "press", side: "bottom", align: "start", gap: 4 }} role="dialog" label={title ?? label} pad={3} gap={2} radius="lg" fill="raised" border="border-strong" shadow="md">
      {title !== undefined && <text style="heading" color="text">{title}</text>}
      {children}
    </col>
  </Button>
);
