# Next session

## Where we are

The concept and architecture are documented. Two isolated Rust/GPUI experiments are retained: [build-loop timing](experiments/gpui-build-loop/README.md) and [the core lifecycle prototype](experiments/core-lifecycle/README.md).

The lifecycle prototype builds a small real core alongside Tone. A Codex subagent then authored Tremolo and its custom GPUI editor from the SDK notes and example, without changing the core. First compile and both extension tests passed. Five integration tests pass across the workspace. Successful/failed code reload and state restoration are verified.

Read [CONCEPT.md](CONCEPT.md), [ARCHITECTURE.md](ARCHITECTURE.md), then the prototype README and SDK.md. [SDK_SKETCH.md](SDK_SKETCH.md) is a hypothesis tested by implementation, not an API to approve before core work.

## Next step

Resolve native repaint validation before expanding the GUI. Automated clicks and file edits update records and trigger observers, but screenshots only show changes after resizing. Accessible controls also remain unverified.

The next UI experiment is a small [design system in the UI SDK](ARCHITECTURE.md#ui-design-system). Use shared styling and accessible controls in Tone, then have a Codex subagent adapt Tremolo with the same components. Verify consistency, keyboard/focus behaviour, accessible labels and simpler extension code. Direct GPUI remains available for custom musical interfaces.

Further core work should test live device output and the control-to-audio handoff against these extensions. Offline sample rendering works; realtime audio does not exist yet. Keep building core capabilities with real extension examples and adjust the SDK from evidence. Do not build the integrated agent yet; Codex subagents can run authoring tests.

Last write wins for file edits, active drags, undo, redo and cancellation. Do not reopen this as a synchronization problem.

## Constraints to carry forward

- An extension is a package providing tools and supporting functionality. A tool can be a small building block or a complete composition workflow. Processor and UI details are internal capabilities, not necessarily separate extensions.
- Focus effort on a strong core. Extensions are trusted user code. Extension correctness and saved-data compatibility are the user's or agent's responsibility; no migration framework or sandbox requirement.
- The core owns audio execution, device selection, transport and a general timeline. Musical conventions, MIDI integration and audio plugin hosting belong to extensions. Do not turn extension examples into core features.
- The outer process manages the agent and builds; the inner Rust runtime owns live project state and audio. One open project at a time.
- One visible window: agent sidebar and musical workspace. GPUI is provisional. Rendering the whole window in the runtime is a proposal to test, not a validated solution.
- Using Pi is acceptable. Users need provider logins and supported subscriptions, not interchangeable coding-agent software. Verify actual provider support.
- Existing code keeps running during builds. Successful builds trigger automatic reload with playback stopped. Seamless code or graph replacement is not required.
- Two speed budgets: extension work compiles and reloads; project work applies instantly and never compiles. Bundled extensions should make most composer requests project edits.
- The project folder is always live and, with the extensions, fully describes the work. The runtime writes records after finished edits and applies an outside file change as a whole-record replace, one typed action and one undo step. Agents edit files, not an edit API. No explicit save; versions via git or snapshots.
- Agent and composer writes use last-write-wins. Undo history resets on close or reload.
- Projects own editable extension copies and imported samples. No extension or schema version numbers; the code in the folder is the version. Later extension updates are explicit copies.
- Target desktop macOS, Windows and Linux; develop and validate primarily on Mac. Sound Tools itself is standalone.

## How to continue the discussion

Ask only about choices that materially change the product or core contracts. Resolve small edge cases using the settled rules, especially last-write-wins. When a question is needed, ask one concrete question at a time. Record accepted decisions in the architecture document as the discussion proceeds. Check the document before reopening a question. Distinguish settled requirements, proposed implementations and work that still needs verification. Keep language simple and avoid adding infrastructure for hypothetical extension mistakes.

## Reference code

Existing local reference repositories:

- `/Users/casperleerink/hooman/reference-repos/pi-mono`: extension registration, UI contributions, SDK/RPC integration, agent access to documentation and examples.
- `/Users/casperleerink/hooman/reference-repos/pure-data`: processor composition, DSP execution and event scheduling.

Useful Pi entry points are `packages/coding-agent/src/core/extensions/`, `packages/coding-agent/src/core/system-prompt.ts` and `packages/coding-agent/examples/extensions/`. Useful Pd entry points are `src/d_ugen.c` and `src/m_sched.c`.

Neither reference needs to dictate the product's UI or musical model. Consult current source and official documentation when selecting concrete dependencies.
