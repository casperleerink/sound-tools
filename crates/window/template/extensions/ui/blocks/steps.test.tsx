import { expect, test } from "bun:test";
import { act, find, h, render, type Card } from "../../sdk";
import { StepRow, type Step } from "./steps";

/** A row of `steps` whose presses and drags its code hears in `heard`. */
const draw = (steps: Step[]) => {
  const heard: string[] = [];
  render(((_self, _ctx) => (
    <StepRow id="kick" label="Kick" steps={steps} onToggle={(i) => heard.push(`toggle ${i}`)} onVelocity={(i, v) => heard.push(`velocity ${i} ${v.toFixed(2)}`)} />
  )) as Card, { name: "drums", state: {}, children: [] });
  return heard;
};
const steps = (count: number): Step[] => Array.from({ length: count }, (_, i) => ({ on: i % 4 === 0, velocity: 0.5 }));

test("a press on a step turns it through its code", () => {
  const heard = draw(steps(16));
  expect(find("Kick step 2")).toMatchObject({ role: "gridcell", checked: false });
  act("Kick step 2", "press");
  expect(heard).toEqual(["toggle 1"]);
});

test("a drag up or down on a step changes how strong it plays, within its whole travel", () => {
  const heard = draw(steps(16));
  act("Kick step 1", "drag", { dy: 15 });
  act("Kick step 1", "drag", { dy: -600 });
  expect(heard).toEqual(["velocity 0 0.25", "velocity 0 1.00"]);
});

test("the steps grow and shrink with what the code keeps", () => {
  draw(steps(8));
  expect([find("Kick step 8") !== undefined, find("Kick step 9") !== undefined]).toEqual([true, false]);
  draw(steps(16));
  expect(find("Kick step 16")).toBeDefined();
});
