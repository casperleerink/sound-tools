import { h, now, Canvas, Space, Stage } from "./sdk";

/** The film the song is scored to, on the stage: the frame at the playhead's second, still while it stops. */
export const Movie = ({ name }: { name: string }) => {
  const room = now.space("movie");
  return (
    <Stage style={{ grow: true }}>
      <Space key="movie" grow>
        <Canvas x={0} y={0} width={room.right} height={room.bottom} across={480}
          pixels={({ video }) => video(name, { at: now.seconds, loops: false })} />
      </Space>
    </Stage>
  );
};
