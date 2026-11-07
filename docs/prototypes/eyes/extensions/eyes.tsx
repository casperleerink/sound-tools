import { abs, h, Label, lowpass, min, mix, noise, now, number, Canvas, saw, smooth, Space, tool } from "./sdk";

tool({
  name: "eyes",
  title: "Eyes",
  when: "You want a video to play a drone: light opens it, movement makes it shimmer",
  doc: "Watches `zoom.mp4`. How bright it is opens the low-pass of a drone of three saws; how much moves from frame to frame brings in a high shimmer. Movement shows red on the picture.",
  state: {},
  inputs: {
    bright: number({ min: 0, max: 1, default: 0 }),
    motion: number({ min: 0, max: 1, default: 0 }),
  },
  sound: ({ bright, motion }) => {
    const drone = saw(55).plus(saw(82.4)).plus(saw(110.3)).times(0.2);
    const open = lowpass(drone, bright.times(bright).times(4000).plus(80), 3);
    // The screen draws more often than the video changes: movement is heard over a while.
    const shimmer = lowpass(noise(), 9000).times(min(smooth(motion, 200).times(12), 1)).times(0.15);
    return open.plus(shimmer).times(0.5);
  },
  view: ({ size }) => (
    <Space grow>
      <Canvas
        x={0}
        y={0}
        width={() => size.width}
        height={() => size.height}
        across={240}
        pixels={({ video, last, measure }) => {
          const frame = video("zoom.mp4");
          const light = frame.red.times(0.3).plus(frame.green.times(0.59)).plus(frame.blue.times(0.11));
          // Green holds the light alone, so the frame before tells what moved.
          const moved = abs(light.minus(last().green));
          measure("bright", light);
          measure("motion", moved);
          return { red: mix(light, 1, min(moved.times(4), 1)), green: light, blue: light };
        }}
      />
      <Label x={16} y={16} text={now.value("bright")} unit="percent" color="#ffffff" size={14} />
      <Label x={16} y={36} text={now.value("motion").smooth(300)} unit="percent" color="#ff8080" size={14} />
    </Space>
  ),
});
