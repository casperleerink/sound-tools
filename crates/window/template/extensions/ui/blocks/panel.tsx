import { h, type Child } from "../../sdk";

/** A surface of its own with a title, such as the devices of a track; `end` sits at the right of the title. */
export const Panel = ({ title, end, children }: { title: string; end?: Child; children?: Child }) => (
  <col pad={4} gap={3} radius="lg" fill="surface" border="border-strong">
    <row h={24} gap={2} align="center" justify="between">
      <text style="title" color="text">{title}</text>
      {end}
    </row>
    {children}
  </col>
);

/** A part of a page under a quiet heading, with no surface of its own. */
export const Section = ({ title, children }: { title: string; children?: Child }) => (
  <col gap={2} align="start">
    <text style="heading" color="text-secondary">{title}</text>
    {children}
  </col>
);
