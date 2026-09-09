# Next session

## Where we are

Sound Tools is a music environment composers shape through an integrated agent. We have discussed and documented the concept and architectural boundaries. No application code or Rust workspace exists yet.

Read [CONCEPT.md](CONCEPT.md), then [ARCHITECTURE.md](ARCHITECTURE.md). These are the source of truth. The first commit is `Document initial concept and architecture`.

## Next step

Make the SDK design concrete with Casper before starting a full implementation. Sketch one small composer-facing tool and trace its lifecycle: registration, creation, connections, edits, saving, reload and deletion. Use an example to test the core contracts, not to prescribe a bundled extension roadmap.

The sketch should answer:

- How does a tool associate its optional state, behaviour, views, actions and ports without one oversized interface?
- How do views and agents edit the same typed state through SDK services?
- How are owned child instances distinguished from references to existing tools?
- How do declared ports and parameters reach the audio engine?
- How does the outer application invoke those operations in the project runtime?

Keep the exact Rust APIs provisional. Identify the smallest prototype that can test them, especially agent-authored GPUI interfaces and build/reload time. Agree its scope before expanding into implementation.

## Constraints to carry forward

- An extension is a package providing tools and supporting functionality. A tool can be a small building block or a complete composition workflow. Processor and UI details are internal capabilities, not necessarily separate extensions.
- Focus effort on a strong core. Extensions are trusted user code. Extension correctness and saved-data compatibility are the user's or agent's responsibility; no migration framework or sandbox requirement.
- The core owns audio execution, device selection, transport and a general timeline. Musical conventions, MIDI integration and audio plugin hosting belong to extensions. Do not turn extension examples into core features.
- The outer process manages the agent and builds; the inner Rust runtime owns live project state and audio. One open project at a time.
- One visible window: agent sidebar and musical workspace. GPUI is provisional. Rendering the whole window in the runtime is a proposal to test, not a validated solution.
- Using Pi is acceptable. Users need provider logins and supported subscriptions, not interchangeable coding-agent software. Verify actual provider support.
- Existing code keeps running during builds. Successful builds trigger automatic save and reload with playback stopped. Seamless code or graph replacement is not required.
- Parameter and other project-state edits need no rebuild. Agent and composer writes use last-write-wins. Undo history resets on close or reload.
- Projects own editable extension copies and imported samples. Later extension updates are explicit.
- Target desktop macOS, Windows and Linux; develop and validate primarily on Mac. Sound Tools itself is standalone.

## How to continue the discussion

Ask concrete questions one at a time until remaining requirements are understood. Record accepted decisions in the architecture document as the discussion proceeds. Check the document before reopening a question. Distinguish settled requirements, proposed implementations and work that still needs verification. Keep language simple and avoid adding infrastructure for hypothetical extension mistakes.

## Reference code

Existing local reference repositories:

- `/Users/casperleerink/hooman/reference-repos/pi-mono`: extension registration, UI contributions, SDK/RPC integration, agent access to documentation and examples.
- `/Users/casperleerink/hooman/reference-repos/pure-data`: processor composition, DSP execution and event scheduling.

Useful Pi entry points are `packages/coding-agent/src/core/extensions/`, `packages/coding-agent/src/core/system-prompt.ts` and `packages/coding-agent/examples/extensions/`. Useful Pd entry points are `src/d_ugen.c` and `src/m_sched.c`.

Neither reference needs to dictate the product's UI or musical model. Consult current source and official documentation when selecting concrete dependencies.
