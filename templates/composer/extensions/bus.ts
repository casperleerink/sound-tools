import { chain } from "./chain";
import { boolean, text, tool, type Declaration } from "./sdk";

const parts = { chain, mix: "fader" } as const;

export const bus = tool({
  name: "bus",
  title: "Return",
  when: "You change a return of the piece: what plays the sends of the instruments, such as a reverb",
  doc: "A return of a `piece`: what the sends of its instruments bring plays through the effects of its `chain` into its fader `mix`. With `pre_fader` the instruments send what comes before their own fader, so one turned down still sends; a muted or soloed-out one sends nothing. A solo never silences a return. `title` is what the piece calls it, `colour` how it draws its dot.",
  state: { title: text(), colour: text({ default: "teal" }), pre_fader: boolean({ default: false }) },
  children: parts,
  behaviour: (): Declaration<typeof parts> => ({
    connections: [
      { from: { port: "audio" }, to: { child: "chain" } },
      { from: { child: "chain" }, to: { child: "mix" } },
      { from: { child: "mix" }, to: { port: "audio" } },
    ],
  }),
});
