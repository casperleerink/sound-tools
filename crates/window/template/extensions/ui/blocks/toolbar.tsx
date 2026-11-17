import { h, type Child } from "../../sdk";

/** A row of small controls (`size="sm"`): one stop of Tab, the arrow keys move between them. Never a Play button: the app's transport plays. */
export const Toolbar = ({ label, children }: { label: string; children?: Child }) => (
  <row key={`toolbar-${label}`} label={label} role="toolbar" group h={32} px={1} gap={1} align="center" radius="lg" fill="surface" border="border">
    {children}
  </row>
);
