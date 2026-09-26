# reverb

The built-in reverb effect. It has one tool, `reverb`, which goes in an effect slot of a track like an effect plugin: an `audio` input and an `audio` output, the port names of an effect in `crates/notes`. The arrangement finds it by those ports and knows nothing else of it.

It follows the pattern of the Filter, ARCHITECTURE.md "Built-in effects".

Enable it in `project.json` under `extensions` as `"reverb"`. A new project has it.

## The record

[agent-doc.md](agent-doc.md) has the record with every field, its range and default, and starting points for sounds. The runtime writes it into every project as `agent-docs/reverb.md`, listed in the map `AGENTS.md`, and a test of the runtime loads its examples, so it is the single source for the format.

The reverb owns no children, so an instance is one file: `<name>.json` in a track folder, named in the track's `effects`. A field you leave out takes its default. The runtime writes every field when it saves. An unknown field or a value out of range does not load. The instance then keeps its last valid state, and the problem names the file and the field.

| Field | Knob | Range | Default | Travel | Shown as |
| --- | --- | --- | --- | --- | --- |
| `pre_delay_ms` | Pre-delay | 0.5 to 250 | 20 | logarithmic | `20 ms` |
| `decay_seconds` | Decay | 0.2 to 60 | 2 | logarithmic | `500 ms`, `2.4 s` |
| `size` | Size | 0 to 1 | 0.5 | linear | `50%` |
| `damping` | Damping | 0 to 1 | 0.5 | linear | `50%` |
| `diffusion` | Diffusion | 0 to 1 | 0.7 | linear | `70%` |
| `low_cut_hz` | Low cut | 20 to 20000 | 100 | logarithmic | `100 Hz` |
| `high_cut_hz` | High cut | 20 to 20000 | 8000 | logarithmic | `8 kHz` |
| `width` | Width | 0 to 1 | 1 | linear | `100%` |
| `mix` | Mix | 0 to 1 | 0.3 | linear | `30%` |

`freeze` is `true` or `false`. The numbers are `Parameter` constants next to `ReverbState` (`sound_core::Parameter`), and `validate`, `Default`, the knobs, the reset and a test of both docs read them.
