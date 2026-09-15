# Concept

## Goal

Create a music production and composition environment with an integrated AI agent. The v0 is a small, calm DAW with an agent sidebar. The agent has the project's full context and tools to assist with production and composition: adding parts, shaping sounds, arranging, mixing and rendering.

The DAW parts are extensions that ship with the product. Composers can later ask the agent to create or adapt extensions for the needs of a piece, but that is not the v0 headline.

## Composer experience

Composers work in a familiar workspace: tracks, clips, a timeline, instruments, effects and a mixer. They play, draw, arrange, record and listen. They ask the agent for help in plain language, and the result appears live in the workspace.

For example: "Add a bass line in bars 5 to 8 that follows the chords on the piano track."

Users need enough musical intent to guide the agent. Programming knowledge is not required. When a request needs a tool that does not exist, the agent can build it as an extension.

## Core and extensions

The core provides reliable audio execution, timing including a musical clock, project storage, shared editing behaviour, a UI SDK, the agent integration, and the ability to build and reload extensions.

Extensions provide instruments, effects, editors and complete workflows. The bundled arrangement, mixer, instruments and effects are extensions built on the same SDK available to user extensions. They give the agent working examples to adapt.

Musical conventions beyond tempo and time are extension choices. The bundled extensions use Western notes and bars; other extensions may use frequency ratios, additive rhythms or free timing, and they can coexist in one project.

Extensions are trusted user code. The project provides clear contracts, documentation and examples; extension authors remain responsible for their extensions' behaviour.

## Starting point

Build the core and the bundled DAW extensions together. Each bundled extension should be small and finished before adding the next. Custom composition workflows and agent-authored tools follow once the bundled set works.

The v0 succeeds when a composer can open a project, make a short piece with the bundled tools, ask the agent for a change in plain language, hear the result without a build, and reopen the work later.

## Inspirations

- Pi: a focused core with extensions the integrated agent can create and modify.
- Ableton Live and Logic: the workspace shape composers already know.
- Pure Data and Max/MSP: reusable building blocks that composers combine into their own musical tools.

Technical architecture and implementation decisions belong in a separate document.
