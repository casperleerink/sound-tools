import { Circle, For, Fragment, h, Label, Line, memo, now, Rect, Space } from "./sdk";
import { ACROSS, clamp, COLOR, LEAD, TEMPO, type Page } from "./song-page";
import { bpmText, pressMark, pressTime, Shades, tempoMarks } from "./song-ruler";

// The tempo lane under the ruler, while its toggle is on: the tempo changes as one line, a step
// at each, with its tempo where it holds long enough to show it. It draws the tempo map; the
// transport changes a tempo.

export const TempoLane = ({ page }: { page: Page }) => {
  const { memory, clock, end, timeline, playhead } = page;
  const marks = memo(() => tempoMarks(page));
  const height = TEMPO - 1;
  const changes = memo(() => clock.tempo_map.tempo_changes.map(({ tick, bpm }) => ({ tick, bpm })));
  // The slowest tempo at its foot, the fastest at its top.
  const yOf = memo(() => {
    const all = changes().map((change) => change.bpm);
    const [low, high] = [Math.min(...all), Math.max(...all)];
    return (bpm: number) => (high > low ? 6 + ((high - bpm) / (high - low)) * (height - 12) : height / 2);
  });
  const chosen = (tick: number) => memory.mark?.kind === "tempo" && memory.mark.tick === tick;
  return (
    <Space key="tempo" height={height} unit={{ x: ACROSS }} extent={() => ({ x: [-LEAD, end()] })} scroll={{ x: -LEAD }} zoom={{ x: [0.1, 8] }} follow={{ x: "timeline" }}
      onPress={pressTime(page, false)}>
      <Shades page={page} height={height} />
      <For each={changes}>
        {(change, index) => {
          const next = () => changes()[index() + 1];
          const y = () => yOf()(change.bpm);
          const text = bpmText(change.bpm);
          // Its tempo stays at the left edge while it holds past it, and shows where it fits.
          const left = timeline.left.max(change.tick);
          return [
            <Line from={() => [change.tick, y()] as const} to={() => [next()?.tick ?? end(), y()] as const} color="gray-800" width={1.5} />,
            () => {
              const after = next();
              return after ? <Line from={() => [after.tick, y()] as const} to={() => [after.tick, yOf()(after.bpm)] as const} color="gray-800" width={1.5} /> : null;
            },
            <Label x={timeline.pointX.times(5).plus(left)} y={() => (y() > 16 ? y() - 14 : y() + 3)} text={text} size={11} color="gray-700"
              opacity={() => left.negate().plus(next()?.tick ?? end()).ge(timeline.pointX.times(text.length * 5.2 + 12))} />,
          ];
        }}
      </For>
      <For each={marks}>
        {(change) => (
          <Rect key={`tempo-${change.tick}`} x={timeline.pointX.times(-3).plus(change.tick)} y={0} width={timeline.pointX.times(7)} height={height}
            color={COLOR.clear} cursor="pointer" onPress={pressMark(page, change.mark)} />
        )}
      </For>
      <Line from={[playhead, 0]} to={[playhead, height]} color="gray-950" width={1} />
      <For each={marks}>
        {(change) => {
          const text = `${bpmText(change.bpm)} bpm`;
          const y = () => clamp(yOf()(change.bpm), 9, height - 9);
          const shown = () => (chosen(change.tick) ? 1 : now.hover(`tempo-${change.tick}`));
          return [
            <Circle x={change.tick} y={() => yOf()(change.bpm)} radius={3.5} color="gray-950" opacity={() => (chosen(change.tick) ? 1 : 0)} />,
            <Rect x={timeline.pointX.times(8).plus(change.tick)} y={() => y() - 9} width={timeline.pointX.times(text.length * 5.6 + 12)} height={18} radius={4}
              color="gray-100" border="alpha/10" borderWidth={1} opacity={shown} />,
            <Label x={timeline.pointX.times(14).plus(change.tick)} y={() => y() - 6} text={text} size={12} color="gray-950" opacity={shown} />,
          ];
        }}
      </For>
    </Space>
  );
};
