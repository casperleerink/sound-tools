# Concept

## Goal

Create a music and sound environment that composers can shape around the needs of a piece. Users describe the tools they want to an integrated AI agent, which creates or adapts extensions for them.

The product should support making instruments, generating material, performing, and composing complete pieces without requiring one musical system or workflow.

## Composer experience

Composers mainly work through custom interfaces created by the agent. They play, draw, arrange, record, and listen, then ask for changes as their ideas develop.

For example: "Give these three voices independent rhythms and let me stretch each pattern by dragging it."

Users need enough musical intent to guide the agent. Programming knowledge should be optional, and the agent can help turn a rough request into something the composer can try.

## Core and extensions

The core provides reliable audio execution, timing, project storage, shared editing behavior, a UI framework, and the agent's ability to build extensions.

Extensions provide musical rules, instruments, effects, editors, and complete workflows. They can connect and combine into larger reusable tools. Composers can use these tools without seeing their internal construction.

Musical conventions remain optional. An extension might use Western note names, frequency ratios, additive rhythms, or free timing. Different approaches should be able to coexist within one project.

Tracks, clips, and arrangement timelines are also extension choices. A tool's musical content exists independently of its interface, allowing compatible views to edit the same work.

Extensions are trusted user code. The project provides clear contracts, documentation, and examples; extension authors remain responsible for their extensions' behavior.

## Starting point

Build the core alongside bundled extensions for instruments and creating material. These use the same SDK available to user extensions and give the agent working examples to adapt. Extensions for complete composition workflows can follow.

The concept succeeds when a composer can ask for a specific tool, make music through its interface, combine it with another tool, and save and reopen the work.

## Inspirations

- Pi: a focused core with extensions the integrated agent can create and modify.
- Pure Data and Max/MSP: reusable building blocks that composers combine into their own musical tools.

Technical architecture and implementation decisions belong in a separate document.
