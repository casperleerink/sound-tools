import { h, type Color } from "../sdk";

const tones = { quiet: "text-secondary", warning: "warning", danger: "danger" } as const satisfies Record<string, Color>;

/** A quiet line that says what happened, such as a file that did not load; it blocks nothing. */
export const Notice = ({ text, tone = "quiet" }: { text: string; tone?: keyof typeof tones }) => (
  <row gap={2} align="center">
    <box w={6} h={6} radius="full" fill={tones[tone]} />
    <text style="label" color={tone === "quiet" ? "text-secondary" : tones[tone]}>{text}</text>
  </row>
);
