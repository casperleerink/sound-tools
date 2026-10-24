# Sound Tools

- Read [ENGINEERING.md](ENGINEERING.md) before writing code.
- Decisions are in [ARCHITECTURE.md](ARCHITECTURE.md).
- Users are composers, not programmers. Text they see follows [DESIGN.md](DESIGN.md): no code words.
- Verify with `cargo verify` (ENGINEERING.md, "Testing"), not workspace-wide commands.
- Run builds and tests in the foreground, never in the background with polling.
- A subagent pushes and returns. It does not wait for CI; the main session watches CI.

## Docs and comments

- A doc holds decisions: what we chose, why, and what we rejected. The code holds the how. If reading the code tells you something, don't write it in a doc.
- When you add a doc line, remove or shorten one. `tooling/workspace-rules` fails when a top-level doc passes its word limit.
- Docs point at code; code never points at docs. `tooling/workspace-rules` checks this too.
- Keep code comments minimal. Write one only when the code cannot explain itself: a reason that is not obvious, a trap, a rule from outside (a plugin format, the OS). Never restate the code; a better name beats a comment.
- When you change code, fix or remove the comments and doc lines it makes wrong.
