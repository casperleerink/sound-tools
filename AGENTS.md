# Sound Tools

- Read [ENGINEERING.md](ENGINEERING.md) before writing code.
- Decisions are in [ARCHITECTURE.md](ARCHITECTURE.md).
- Run builds and tests in the foreground. Don't start them in the background and poll the output in a loop.
- A subagent pushes and returns. It does not wait for CI; the main session watches CI.
