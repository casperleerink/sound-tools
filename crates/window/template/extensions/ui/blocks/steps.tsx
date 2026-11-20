import { h, live } from "../../sdk";
import { tint } from "../control";

export type Step = { on: boolean; velocity: number };
const [W, H, TRAVEL] = [28, 36, 60];

/**
 * A row of steps `ticks` apart: a press turns one on or off, a drag up or down sets how hard it
 * plays, and the step the piece plays is marked green while it plays.
 */
export const StepRow = ({ id, label, steps, ticks = 240, onToggle, onVelocity }: { id: string; label: string; steps: readonly Step[]; ticks?: number; onToggle: (step: number) => void; onVelocity: (step: number, velocity: number) => void }) => {
  const now = live.playhead.over(ticks).floor().mod(steps.length);
  return (
    <row gap={1}>
      {steps.map((step, i) => {
        const key = `${id}-${i}`;
        const velocity = live.drag(key).dy.over(-TRAVEL).plus(step.velocity).clamp(0, 1);
        return (
          <box key={key} label={`${label} step ${i + 1}`} role="gridcell" checked={step.on} value={Math.round(step.velocity * 100)} w={W} h={H} radius="sm" fill={step.on ? tint("on") : "fill"} hover="fill" border={i % 4 === 0 ? "border-strong" : undefined} clip
            onPress={() => onToggle(i)} actions={{ toggle: () => onToggle(i), set: (to) => onVelocity(i, Math.min(Math.max(to, 0), 1)) }}
            drag={{ y: [-(1 - step.velocity) * TRAVEL, step.velocity * TRAVEL] }} onDragEnd={({ dy }) => onVelocity(i, Math.min(Math.max(step.velocity - dy / TRAVEL, 0), 1))}>
            {step.on && <box at={[4, velocity.times(-(H - 8)).plus(H - 6)]} w={W - 8} h={2} radius="full" fill="text" />}
            <box at={[0, H - 2]} w={W} h={2} fill="sound" opacity={now.eq(i).times(live.playing)} inert />
          </box>
        );
      })}
    </row>
  );
};

/** Rows of steps, such as the drums of a kit: one stop of Tab, the arrow keys move between the steps. */
export const StepGrid = ({ id, label, rows, ticks, onToggle, onVelocity }: { id: string; label: string; rows: readonly { name: string; steps: readonly Step[] }[]; ticks?: number; onToggle: (row: number, step: number) => void; onVelocity: (row: number, step: number, velocity: number) => void }) => (
  <col key={id} label={label} role="grid" group gap={1}>
    {rows.map((row, r) => (
      <row gap={2} align="center">
        <text style="label" color="text-secondary" w={64} truncate>{row.name}</text>
        <StepRow id={`${id}-${r}`} label={row.name} steps={row.steps} ticks={ticks} onToggle={(step) => onToggle(r, step)} onVelocity={(step, velocity) => onVelocity(r, step, velocity)} />
      </row>
    ))}
  </col>
);
