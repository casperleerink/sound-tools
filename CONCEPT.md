# Concept

## Goal

Create a music production and composition environment with an integrated AI agent. The v0 is a small, calm DAW with an agent sidebar. The agent has the project's full context and tools to assist with production and composition: adding parts, shaping sounds, arranging, mixing and rendering.

The DAW is a template a new project starts from, written as tools of the project with the TypeScript SDK, so a composer can ask the agent to change any of it. A Bare project starts empty.

## Composer experience

Composers work in a familiar workspace: tracks, clips, a timeline, instruments, effects and a mixer. They play, draw, arrange, record and listen. They ask the agent for help in plain language, and the result appears live in the workspace.

For example: "Add a bass line in bars 5 to 8 that follows the chords on the piano track."

Users need enough musical intent to guide the agent. Programming knowledge is not required. When a request needs a tool that does not exist, the agent can build it as an extension.

## Core and extensions

The core provides reliable audio execution, timing including a musical clock, project storage, shared editing behaviour, a UI SDK, the agent integration, and running the tools a project defines itself.

Extensions provide building blocks in Rust: instruments, effects, the plugin host, clips in time, faders and meters. Tools of the project compose them into workflows. The DAW template is such tools, written with the SDK an agent writes with, so it is a working example to adapt.

Musical conventions beyond tempo and time are extension choices. The bundled extensions use Western notes and bars; other extensions may use frequency ratios, additive rhythms or free timing, and they can coexist in one project.

Extensions are trusted user code. The project provides clear contracts, documentation and examples; extension authors remain responsible for their extensions' behaviour.

## Where it stands

The DAW comes first: a composer can make a short piece with the bundled tools and reopen it later. The agent sidebar runs Claude Code in the project folder. The app downloads it and signs in through Claude Code's own browser flow, and each message is one point of history. A coding agent in a terminal still works the same way, since both only edit the project files.

A composer can also ask for a tool that does not exist: the agent writes it in TypeScript in the project folder, and it plays at once, with no build.

## Inspirations

- Pi: a focused core with extensions the integrated agent can create and modify.
- Ableton Live and Logic: the workspace shape composers already know.
- Pure Data and Max/MSP: reusable building blocks that composers combine into their own musical tools.

Technical decisions are in [ARCHITECTURE.md](ARCHITECTURE.md).
