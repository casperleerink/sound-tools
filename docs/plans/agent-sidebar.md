# Plan: the agent sidebar

## Summary for Casper

- **Approach.** v1 runs Claude Code only. Rust talks to the `claude` CLI directly in stream-json mode, with no Node and no sidecar. A spike ran a real turn, an approval and a resume in about 60 lines of script.
- **More providers later.** Everything outside one driver module is provider-neutral: events, threads, approvals, install, sign-in states and the UI. Adding Codex is one new module. It is already spiked, and the notes are at the end of this file.
- **Lean agent.** Claude Code runs trimmed to 6 tools (Bash, Read, Edit, Write, Glob, Grep), with no MCP servers and none of the user's own setup. It still uses the Claude subscription.
- **Install and sign-in.** On first use the app downloads a pinned, checksum-checked `claude` (215 MB) into Application Support. Sign-in is Claude Code's own browser flow, and the app never sees a token. No terminal.
- **UI.** gpui's `list` for the thread, a small markdown renderer of our own, and our `TextInput` made multi-line. The sidebar collapses and the app remembers whether it is open.
- **Undo.** The sidebar says where each request begins and ends, so one request is one undo step.
- **Decided.** Approvals are a user setting, with "Ask before commands" as the default. Sound Tools stays open source.

---

## Goal

An agent sidebar on the left of the window. The composer talks to Claude Code there, and the agent works in the open project folder as the terminal agents do today. A composer who has never opened a terminal can install, sign in and ask for a change. Adding another provider later, such as Codex, changes one module of the agent crate.

## Decided constraints

- v1 has one provider, Claude Code, with the composer's Claude subscription or Anthropic Console account. Only the unmodified Claude Code binary may use a Claude subscription, so the agent is Claude Code and not a lighter agent.
- The code keeps a provider seam: one `Provider` enum and one driver module per provider. Nothing outside the driver names a Claude protocol type.
- The agent's cwd is the project folder. It edits files and the runtime applies them live. There is no new edit API.
- The composer never needs a terminal. The app installs the CLI and runs its own sign-in.
- `sound-core` and `sound-ui` never depend on the agent code (ARCHITECTURE.md, "Direction"). Anything the sidebar needs from them, such as request boundaries, is generic and usable by any agent.
- UI follows DESIGN.md: the quiet rule, dark only, lavender means the agent, nothing blocks. Components come from `crates/ui`. No gpui-component.
- Chat and interface state stay out of the project folder. The folder is only what the agent writes.
- Credentials stay in Claude Code's own store. The app never reads, stores or forwards tokens.
- How much the agent may do without asking is a user setting (section 5).
- The sidebar collapses. The app remembers whether it is open, per machine and not per project (section 6).
- The two remembered settings live in the machine's support folder, never in a project. An agent that works in the project folder must not be able to give itself more access by editing a file there.
- Sound Tools stays open source and local.
- The hooman-studio runner (`~/hooman/hooman-studio/apps/runner`) is the reference for behaviour and edge cases. We port its logic to Rust and do not run it.

## What the spikes showed

Throwaway Python scripts in `/tmp`, run against claude 2.1.286 on this Mac. None of them is committed.

| Spike | Result |
| --- | --- |
| `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --permission-prompt-tool stdio` | Works. The `initialize` control request returns `account` (email, plan) and `models`. A Write triggers `control_request can_use_tool`, and our `allow` reply lets it through. The turn ends with a `result` message. |
| The same with `--tools Bash,Read,Edit,Write,Glob,Grep --strict-mcp-config --setting-sources project,local --disable-slash-commands` | Works on a Claude Max subscription with Opus 5.5. It had those 6 tools and no MCP servers, and it edited a file with Read then Write. |
| `claude --resume <session-id>` in stream-json | Works. The agent remembered the earlier turn. |
| `claude auth status --json` | Returns `loggedIn`, `authMethod` and `subscriptionType`. Exit code 1 when signed out. |
| `claude auth login` with no terminal, in a throwaway `CLAUDE_CONFIG_DIR` | Opens the browser and listens on a localhost port for the callback, so sign-in completes without pasting a code. It also prints a fallback URL and a "paste code" prompt. A real sign-in to the end was not done (R2). |
| `claude --bare` | Not usable. It never reads the subscription login, only `ANTHROPIC_API_KEY`. |
| Download | `install.sh` fetches `https://downloads.claude.ai/claude-code-releases/<version>/<platform>/claude` (215 MB) and checks the sha256 in `manifest.json`. We can do the same, unmodified. |
| Agent SDK (TS, 0.3.273) | It starts the same binary with the same flags and sends the same control messages (about 45 kinds). It adds types, docs, in-process tools and hooks, and helpers that read transcript files. We need none of these for v1. |
| How others install | The Claude desktop app downloads Claude Code into `Application Support/Claude/claude-code/<version>`. Zed downloads its own Node and ACP adapters. T3 Code uses the CLIs on PATH and asks the user to install them. Conductor bundles them and reuses the Mac's login. |
| Milestone 3: `interrupt` (R1) | Works. The CLI acknowledges it at once (`still_queued: []`), sends the partial text as an `assistant` message and a user note "[Request interrupted by user]", and ends the turn with a `result` of `error_during_execution` and `terminal_reason: aborted_streaming`. The streamed text block is never closed. The same process takes the next message. |
| Milestone 3: interrupt during a question | The CLI sends `control_cancel_request` for the open `can_use_tool`, a rejected `tool_result` and a `result` with `stop_reason: tool_use`. The driver ends the turn as interrupted and voids the question. The CLI then exits with code 1 when stdin closes. |
| Milestone 3: denied tool (R1) | Works. A `deny` answer comes back as a `tool_result` with `is_error: true` and our message, and the turn goes on to a normal `result`. |
| Milestone 3: error turn (R1) | A model that does not exist: the CLI writes the error as an `assistant` text with no streaming, then a `result` with `is_error: true` and `terminal_reason: api_error`. |
| Milestone 3: stale resume | `--resume` with an unknown id: no answer to `initialize`, a `result` with `errors: ["No conversation found with session ID: …"]`, the same line on stderr, exit code 1. It comes at once, before any message is sent. The driver ends with `Exited { SessionNotFound }`, and the sidebar turns that into "start a new thread". |
| Milestone 3: killed process | Stdout ends mid-stream with no `result`, and no stderr. The driver ends the open turn as failed. |
| Milestone 3: `set_permission_mode` (R10) | Works between `default` and `acceptEdits` on a running process, and applies to the next tool. Switching to `bypassPermissions` fails ("the session was not launched with --dangerously-skip-permissions") unless the process started with `--allow-dangerously-skip-permissions`. That flag only allows the switch; the mode stays the one `--permission-mode` gives. So the driver always passes it, and no restart is needed. |
| Milestone 3: trimmed flags and `CLAUDE.md` | The project's `CLAUDE.md` loads, and its `@AGENTS.md` import too: the live test asks for a fact that is only in `AGENTS.md`. `initialize` answers without a user message. |
| Milestone 3: settings in the project | With `--setting-sources project,local`, a `.claude/settings.json` the agent could write itself allows `Bash` without asking, and its `SessionStart` hook runs a command at the next start. `--setting-sources ""` ignores both, but then `CLAUDE.md` does not load either. `--restricted` also drops `CLAUDE.md`, and refuses `bypassPermissions`. `--safe-mode` drops `CLAUDE.md` by design. What works: `--setting-sources "" --add-dir <project>` with `CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD=1`. `CLAUDE.md` and its import load, no settings file or hook does, and all three approval modes behave. An ignored live test checks it. |
| Milestone 6: `set_model` | A control request, `{"subtype": "set_model", "model": <id>}`, on a running process. It applies from the next message (the one in a turn that runs keeps its model), with no restart; haiku to sonnet and back to `default` all worked. An unknown model gets an error answer ("Model '…' not found") and the model stays. Recorded in `set_model.jsonl`. |
| Milestone 6: read-only commands | Under `acceptEdits` the CLI runs `ls`, `cat`, `head`, `tail`, `wc`, `find`, `grep` and `rg` with no question, also in a pipe, after `&&` and with absolute paths. Under `default` `ls` runs with no question too. What asks: `find … -exec`, and a loop over a command substitution, which is what the milestone 4 run met (`for f in $(find state -name '*.json'); do cat "$f"; done`). `--allowedTools "Bash(find:*)"`-style rules change none of this, so the app adds none. Recorded in `read_only.jsonl`. |

## 1. Process model

**Recommendation: Rust speaks to the `claude` CLI directly.** It is newline-delimited JSON over stdio, with an explicit end of turn and approval requests that we answer.

| | Rust direct (chosen) | Node/Bun sidecar with the TS Agent SDK | ACP (`agent-client-protocol` 2.2.0 plus adapter) |
| --- | --- | --- | --- |
| Bundle size | +0. CLI downloaded on use | +60 MB for a compiled Bun sidecar, and the SDK still runs the 215 MB `claude` | Node plus the npm adapter, and still the `claude` binary |
| Node on the user's machine | No | Bundled or required | Bundled (as Zed does) |
| Crash isolation | The CLI is a child process, so a crash is a failed turn. Our driver follows the no-panic rules | The same, plus one more process | The same, plus the adapter |
| hooman code reuse | Port the logic of its Claude driver and event mapping (about 1.2k lines) | Lift the TS files, then write our own protocol between Rust and the sidecar | None |
| Types | serde enums, checked by the compiler | Types stop at the process edge | Types from the crate, but provider details hide in `_meta` |

- Rejected: **sidecar.** It adds a runtime and our own IPC protocol, and it saves only the mapping code, which is where the Rust types help most.
- Rejected: **ACP now.** ACP is a protocol, not an agent. In Zed, Claude through ACP runs through Node, then the `claude-agent-acp` adapter (0.84.0, npm-only), then the Agent SDK, then the same `claude` binary. It adds layers and removes none. ACP is the right way to add many agents later (Gemini CLI, or pi with an API key), as one more `Provider` variant.
- Rejected: **a lighter agent such as pi, running Claude models.** Anthropic does not allow third-party apps "to route requests through Free, Pro, or Max plan credentials". A lighter agent would need an API key, so we trim Claude Code with flags instead.
- Rejected: **the unofficial `claude-agent-sdk` Rust crate.** It is maintained by a private person and wraps the same undocumented messages.

How it runs:

- There is one child process per open thread. Use `smol::process::Command` with `kill_on_drop` (clippy already requires smol).
- The stdout reader runs on the background executor. It parses each line into a typed message and hands a batch to the sidebar entity once per frame.
- Every protocol enum has an `Unknown` catch-all (`#[serde(other)]` or an untagged fallback). A new message kind from the CLI is then ignored, not a crash.
- The CLI exits when stdin closes, so quitting the app or "Open project…" also ends it.
- The environment comes from the login shell (`$SHELL -ilc 'env -0'`, captured once in the background at start), as hooman's `shell-env.ts` and Zed do. An app opened from the Finder has a bare PATH, and the agent needs `cargo` and `git` to build extensions.
- Remove `CLAUDECODE`, `CLAUDE_CODE_*` and `ELECTRON_RUN_AS_NODE` from that environment. hooman found that a nested session never saves its transcript. Set `DISABLE_AUTOUPDATER=1`.
- Flags: `-p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --permission-prompt-tool stdio --permission-mode <mode> --allow-dangerously-skip-permissions [--resume <id> | --session-id <uuid>] --model <m>`.
  - Pass every flag explicitly, because the docs say `-p` defaults will change.
  - `--allow-dangerously-skip-permissions` lets a running process switch to "Never ask" (R10).
  - The `capabilities` list of `system/init` (`interrupt_receipt_v1`, `msg_lifecycle_v1` and MCP entries on 2.1.286) names nothing the driver uses, so the driver checks none. The pin and the fixtures are the guard.
- Trimmed to what a composer needs: `--tools Bash,Read,Edit,Write,Glob,Grep --strict-mcp-config --setting-sources "" --disable-slash-commands --add-dir <project>`, with `CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD=1`.
  - The composer's own user-level setup (MCP servers, skills, hooks, plugins) does not load. Every composer gets the same agent, and a slow MCP server cannot delay the start.
  - No settings file loads, not even the project's. The agent writes in the project, and a permission rule or a hook there would give it more access than the approval mode.
  - The project comes back as an added folder only so its `CLAUDE.md` (the map) loads (milestone 3).
  - `AskUserQuestion` is not in the list, so the agent asks its questions in plain text.
- The default config folder (`~/.claude`) stays, so a login made in a terminal or in the Claude app is shared.

## 2. Install and sign in

**Recommendation: the app keeps its own pinned copy of `claude` and runs Claude Code's own sign-in.** Changed later: a `claude` on the login shell's `PATH` comes first, and the download is for those without one (see `ARCHITECTURE.md`).

Install:

- The version and the sha256 of each platform (macOS arm64 and x64, Linux x64 and arm64) are constants in the Claude driver. Moving the pin means changing the constants and recording the fixtures again, in the same commit.
- The download is `downloads.claude.ai/claude-code-releases/<version>/<platform>/claude`, unmodified, into `support_folder()/agents/claude/<version>/`. This is what the Claude desktop app does.
- Use `/usr/bin/curl` (with `-C -`, so a broken download resumes), then check the sha256 with the `sha2` crate. That needs no HTTP or TLS dependency. An older version's folder is removed once the new one checks out.
- The download itself is provider-neutral: a pinned URL and a checksum. Codex later fills in its own values; its asset is a `.tar.gz`, so it adds an unpack step then.

Sign-in:

- `claude auth status --json` tells whether the Mac is signed in. When it isn't, the app offers two choices, both of which run Anthropic's own flow in the browser:
  - "Sign in with your Claude plan" runs `claude auth login --claudeai`.
  - "Use an Anthropic Console account (API)" runs `claude auth login --console`.
- The app waits for the child to exit, then checks the status again. It has no key field, so it never handles a credential.
- Credentials live where Claude Code puts them: the Keychain, or `~/.claude/.credentials.json`.
- Each provider supplies its own list of sign-in choices. The onboarding view shows whatever the provider offers.

Terms (checked 2026-09-30, quotes in Sources):

- Claude Code is proprietary. Running it inside a product requires agreeing to the Commercial Terms, an unmodified binary, every built-in sign-in method kept, and each user paying under their own account.
- Third-party apps may not offer their own "Claude login" or touch tokens. The terms do allow "an end user signing in to the unmodified Claude Code binary with their own Claude subscription", and our design follows that line.
- We may write "runs Claude Code" in plain text in the UI. We must not name a feature after it.

What the composer sees in the sidebar, with nothing blocking the rest of the app:

| State | Sidebar shows |
| --- | --- |
| Not installed | "The agent runs Claude Code by Anthropic. Setup downloads 215 MB, then opens your browser to sign in." and **Set up** |
| Downloading | A progress line with MB and percent, and **Cancel** |
| Download failed | One sentence ("Could not download Claude Code. Check the internet connection.") and **Try again**. A checksum mismatch says "The download was damaged" and deletes the file. A region block repeats Anthropic's message |
| Signed out | The two sign-in choices |
| Signing in | "Finish signing in in your browser.", **Open the page again** (starts the sign-in again, see R2) and **Cancel**. After 10 minutes it goes back to signed out |
| Sign-in failed or cancelled | Back to signed out, with one line saying why |
| Ready | The thread. The account (email and plan) sits in the composer's menu, which also has **Sign out** |
| CLI fails to start | "Claude Code stopped: <first line of its error output>" and **Try again** |

Rejected:

- **Bundling the binary in the app.** It adds 215 MB to every download, and moving the version would need an app release anyway.
- **Using PATH only (the T3 Code way).** It needs a terminal to install, and the CLI updates daily under us.
- **Running `install.sh`.** It writes `~/.local/bin` and the shell profile, and it installs "latest" rather than the version we tested.

## 3. Chat UI in gpui

**Recommendation: gpui `list` for the thread, our own small markdown renderer, and a multi-line `TextInput` in `crates/ui`. We port nothing.**

What gpui v1.20.2 gives us (`crates/gpui/src/elements/list.rs`):

- `ListState` handles items of different heights. `splice` adds a new entry, and `remeasure_items(range)` handles a message that grows while streaming; its doc names streaming as the use.
- `FollowMode::Tail` keeps the view at the bottom, stops following when the composer scrolls up, and follows again once back at the bottom. Zed's agent panel uses `ListAlignment::Top` with `FollowMode::Tail`.
- `uniform_list` measures one row and gives every row that height, so it does not fit messages. It is fine for a thread list later.
- There is no text input element. `crates/ui/src/components/text_input.rs` (790 lines) is a single-line copy of gpui's input example, with IME, selection and clipboard.

What we build:

| Piece | How | Size |
| --- | --- | --- |
| Thread list | `list` with Top alignment and `FollowMode::Tail`. Text deltas are batched once per frame, then `remeasure_items` runs on the growing item | ~150 lines |
| Markdown | `pulldown-cmark` 0.13.4 turns events into a small block model (paragraph, heading, list, quote, code block) with inline runs (bold, italic, code, link), drawn with `StyledText`. The streaming message is parsed again on each frame's batch. Tables render as monospace text. No syntax colours and no selection in v1 | ~500 lines |
| Steps | One line per tool step, such as "Edited state/arrangement/bass/verse-a.json" or "Ran cargo build". The newest step is the working line. After the turn the steps fold behind "Worked for 12 s" | ~150 lines |
| Composer | Extend `TextInput` to multi-line: shape with `shape_text` and a wrap width, move up and down across wrapped lines, grow up to 8 lines and then scroll, keep newlines on paste, simple undo. Enter sends, shift-enter adds a newline, and up in an empty composer recalls earlier messages of the thread. It is a general component in `crates/ui` with a gallery entry | ~800 lines |
| Approval row | See section 5 | ~120 lines |

There are no diffs in v1. The change is already live in the arrangement, the composer hears it, and one cmd-z takes the whole request back. The sidebar has no avatars, no timestamps and no tool rows.

Rejected:

- **Porting from Zed's `agent_ui` or `markdown`.** They are GPL-3.0, and they depend on `editor`, `language` and `workspace`.
- **Porting from gpui-component.** Its input is 28k lines, a code editor on a rope, and its markdown view is 24k lines. Its `MessageScroller` (508 lines) is a thin wrapper on `list`; we take its "jump to bottom" idea and write our own.

## 4. Threads and state

**Recommendation: Claude Code keeps the conversation. The app keeps a small display log per thread in the machine's support folder.**

- The store is `support_folder()/agent/threads/<project key>/`. The project key is a UUID v5 of the canonical project path, so a symlink finds the same threads and any path gives a short folder name.
  - `index.json` lists each thread (its id and provider session id) and names the current one. **+** sets no current thread, so a reopen before the next message shows an empty thread.
  - Each thread has one `<thread id>.jsonl`. Every line is the composer's message or an `AgentEvent`, with the time it came. Text deltas and the start are left out: `TextDone` has the whole text, and the start shows nothing. A write ends a line a crash cut off before it appends, so only the cut line is lost. Opening replays the lines through `Conversation::apply`, which gives the same conversation, "Worked for" included. A turn still open at the end is one the app quit during, and shows as stopped.
- A thread is resumed with the saved provider session id. The app picks the id itself (`--session-id <uuid>` for Claude) and saves it with the first message, before the agent answers, then passes `--resume <uuid>` later.
- The display comes from our log, so reopening needs no provider API and works the same for any provider.
- If a resume fails (the CLI says "no conversation found"), the old log stays visible, read-only, with "This thread can't continue. Start a new one."
- A project can have many threads. In v1 the sidebar shows the current thread, and **+** starts a new one. Older threads stay in the index for a thread list later.
- The model is one setting of the machine, like the approval mode, picked from the `models` of `initialize`. A change goes to the running agent at once. (The plan first had it per thread; one remembered setting is simpler and enough.)
- One turn runs at a time per project, because every turn writes the same folder and makes one undo step.
- While a turn runs, the send button becomes stop, and cmd-period stops too. Both send the `interrupt` control request.
- A thread's process starts on the first send. It ends when a new thread starts, the project closes or the app quits.

Rejected:

- **In the project folder.** The log is machine and interface state, it would be noise in git, and the agent would read its own chat.
- **Only Claude Code's own store.** Stream-json has no "read past messages" call, so we would have to parse its internal JSONL, and each later provider stores its history differently.
- **SQLite, as in hooman.** A JSONL file per thread is enough for one composer.

## 5. Links to the app

**Undo.** Add request boundaries to the core, with no agent in them:

- Add `Project::begin_request(label)` and `Project::end_request()`, with `Session` wrappers.
- While a request is open, every outside change joins one undo step with that label, however far apart the writes are.
- After `end_request`, outside changes the watcher hears less than its 100 ms grouping window later still join the step, so a write that lands just after the turn ends still joins.
- A window edit or an undo during a request splits it, as today (`History::push` clears the grouping).
- The label is the composer's message, cut to about 40 characters, so the menu says "Undo Add a bass line in bars 5 to 8…".
- The 15 s heuristic (`OUTSIDE_UNDO_WINDOW`) stays for outside changes made with no request open, which covers terminal agents.
- Begin is called at send. End is called on `AgentEvent::TurnEnded`, which the driver sends on Claude's `result` and when the process dies.

**Live status.** While working, the sidebar shows the lavender pulsing `Indicator` and the newest step as one line. The arrangement needs nothing new, because the watcher already applies each file as it lands.

**Problems.**

- The sidebar reads `Project::problems()` when a turn begins and when it ends.
- If the turn left problems that were not there before, a peach line goes under the result ("2 files are not live"). It expands to `path: message` lines.
- The agent docs already tell the agent to check `problems.txt`, so v1 sends nothing back automatically.
- The window's corner notice stays as it is.

**Approvals.** How much the agent may do without asking is a user setting with three choices:

| Setting | What the composer reads | Claude `--permission-mode` |
| --- | --- | --- |
| Ask for everything | "Asks before every edit and command, except plain reads like ls." | `default` |
| Ask before commands (default) | "Edits freely, asks before commands, except plain reads." | `acceptEdits` |
| Never ask | "Does anything without asking. Undo and git are your safety net." | `bypassPermissions` |

- The setting is one provider-neutral `enum ApprovalMode`, saved in `support_folder()/agent/settings.json`. Each driver maps it to its own flags with one exhaustive `match`.
- It applies to every project on the machine. Because it is not in the project, the agent cannot change its own permissions by editing a project file.
- It sits in the composer's menu as one select, next to the model.
- A change applies from the next action of the agent, through the `set_permission_mode` control request, with no restart (R10).
- Under "Ask before commands" Claude Code still runs file commands such as `touch` and `mkdir` in the project without asking, as it counts them as edits. Commands such as `git init` or `cargo build` ask. Decided: that is fine.
- Under "Never ask", the first message of a thread shows one quiet line above the composer ("The agent does anything without asking"), so the mode is never a surprise.

An approval request shows as the last item of the thread:

- One sentence of what the agent wants to do, for example "Run `cargo build`", with **Allow**, **Allow for this thread** and **Deny**, in lavender.
- The buttons are tab stops with focus rings. Nothing is modal, and music and editing go on.
- The answers map to Claude's `allow` and `deny`. For "this thread", the app returns the CLI's `permission_suggestions` scoped to the session. When they hold rules, only the rules: for a command the CLI also suggests switching the mode to `acceptEdits`, which "allow this command" should not do.

Rejected:

- **A modal dialog.** DESIGN.md says nothing blocks.
- **A setting per project or per thread.** One setting per machine is enough, and it keeps approvals out of the folder.

## 6. Scope of v1

In v1:

- Claude Code, set up and signed in from the sidebar, with a model picker.
- One visible thread per project, resumed on reopen, and **+** for a new one.
- Streaming markdown answers, steps behind "Worked for", stop, inline approvals, the problems line, and one undo step per request.
- A multi-line composer with history.
- The approval setting (section 5).
- A collapsible sidebar:
  - One sidebar icon in the title row, right of the traffic lights, opens and closes it.
  - cmd-L (free today) opens it and focuses the composer. Pressed again in the composer, cmd-L closes it.
  - Escape in the composer gives the focus back to the arrangement.
  - Closed means gone, with no rail. The arrangement takes the full width.
  - While the sidebar is closed and a turn runs or an approval waits, the icon carries the small lavender `Indicator`, so a closed sidebar never hides a question.
  - The app remembers open or closed for the machine, not the project, in `support_folder()` next to `last-project`. On first start the sidebar is open.
  - This is an exception to DESIGN.md's "interface state is not saved". That rule keeps settings out of the project folder, where an agent could change them, and this setting stays out of it too. Update the rule's wording in the same change.

Later, in rough order of value:

- Codex as a second provider (see "Adding Codex later").
- A thread list.
- Mentions of the selected clip or track. They are cheap, since an id is a path.
- Dropping audio files into the composer.
- Copying and selecting message text.
- An effort picker.
- Steering a running turn.
- More providers through ACP.

Diffs come only if composers ask for them.

## Shape of the code

- A new crate, `crates/agent` (package `sound-agent`). It depends on `sound-core`, `sound-ui` and gpui. Nothing depends on it except the composition root and the gallery.
- `provider/mod.rs` holds the provider seam. It is the only place a new provider touches outside its own module:
  - `enum Provider { Claude }`.
  - The provider-neutral `AgentEvent`: started, turn started, text delta, text done, step started or done, approval requested, turn ended with an outcome, error, process exited.
  - `ApprovalMode`, the sign-in choices, and the pinned download description.
  - Functions that `match` on `Provider` and call the driver: start or resume a thread, send, interrupt, answer an approval, set the approval mode or the model, check the account, sign in and out.
- `provider/claude/` is private and holds every Claude type and flag: `mod.rs` runs the process, `protocol.rs` has the message types, `mapper.rs` turns them into `AgentEvent`s with no I/O, and `setup.rs` has the pinned download and `claude auth`. Adding a provider means a new variant and a new private module. The compiler then points at every `match` that needs a new arm.
- The rest of the crate:
  - `install.rs` downloads from a pinned description.
  - `conversation.rs` builds the thread state from events.
  - `store.rs` keeps the threads, and `settings.rs` the approval mode and the model, in the support folder.
  - `environment.rs` reads the login shell's environment.
  - `view/` holds the sidebar (`sidebar.rs`), the onboarding (`onboarding.rs`), the entries of the thread (`entry.rs`), the composer's menu (`menu.rs`) and its history (`history.rs`).
- Keep the driver apart from the thread state. Tests can then feed `AgentEvent`s to the thread and the view with no process.
- The window gets a generic left panel slot: a GPUI global holding a constructor from `Entity<Session>` to `AnyView`. It is filled in `crates/runtime/src/lib.rs`, where views are registered.
- The runtime owns the open-or-closed flag and the title-row icon, because they belong to the window's generic left panel. The agent crate gives the slot a way to say "working or waiting" for the icon's `Indicator`. `window.rs` names no agent type.
- The sidebar has a fixed width of 360 pt. When the window is narrower than `MIN_WINDOW_WIDTH` plus 360, opening the sidebar widens the window. If the screen has no room, the window stays as it is and the arrangement scrolls.
- The multi-line `TextInput` is general, so it goes in `crates/ui` with a gallery entry. The sidebar is not general, so it stays in `crates/agent`, and so does the markdown renderer, which only the sidebar uses.

## Risks and the spike that removes each

| # | Risk | Spike |
| --- | --- | --- |
| R1 | Claude's control messages are undocumented and could change | Removed for 2.1.286: initialize, can_use_tool, resume, interrupt, a denied tool and an error turn are recorded in `crates/agent/tests/fixtures/claude/`, with snapshot tests. Moving the pin means running `record.py` again and reading the snapshot changes |
| R2 | `claude auth login` without a terminal might not finish | Partly removed. Milestone 5: the pinned 2.1.286 downloads and checks out, `auth status` in a fresh `CLAUDE_CONFIG_DIR` says signed out (exit 1), and `auth login` started by the app with stdin empty listens on `127.0.0.1` and waits; Cancel kills it (ignored test `crates/agent/tests/setup.rs`). The fallback URL it prints is the paste-a-code flow, which the app cannot finish, so **Open the page again** starts the sign-in again instead. Still to do: finish a real sign-in into a throwaway `CLAUDE_CONFIG_DIR`, check exit code 0 and `auth status`, and check the Keychain item with the default folder |
| R4 | An app opened from the Finder has a bare PATH, so the agent cannot find `cargo` | Launch the `.app` from the Finder and have the agent run `which cargo`, with and without the login-shell environment. Partly removed: every run of `claude` gets the login shell's environment (`environment.rs`, with tests), and the sidebar says so when the shell does not answer. Still open, for a person: launch the `.app` from the Finder and check that the agent finds `cargo` |
| R5 | The agent's last write lands after the turn ends and misses the undo step | A core test with `apply_outside_changes`: a write 50 ms after `end_request` joins the step, and a write 500 ms after starts a new one. Removed: `a_write_heard_just_after_the_end_joins_the_request` and `a_write_heard_well_after_the_end_is_a_step_of_its_own` in `crates/core/tests/project/undo_grouping.rs`, and a window test of a write heard just after the turn |
| R6 | The multi-line composer is the largest UI piece | A gallery spike, before the rest: wrapping, up and down across wrapped lines, and growth to 8 lines. Removed: the multi-line `TextInput` in `crates/ui`, with key tests in `crates/ui/tests/text_input.rs`, a gallery entry, and the full composer in `agent-composer-full.png` |
| R7 | Parsing a long streaming message on every frame is too slow | Stream a 5 KB answer into a 200-message thread and check frame times in a release build. Partly removed: a 5.1 KB answer parses in 58 µs in release, against a 16 ms frame (ignored test in `crates/agent/src/view/markdown.rs`), and the sidebar parses an answer again only when its entry changed. Still open, for a person: frame times while a long answer streams in the app |
| R9 | Terms change, or Sound Tools stops being open source | Not a spike. Check the terms page before each release |
| R10 | Claude cannot change its permission mode on a running process | Removed: `set_permission_mode` works on 2.1.286. Switching to "Never ask" needs `--allow-dangerously-skip-permissions` at start, which the driver always passes |

## Milestones

Each milestone ends green on the README checks, with the docs updated in the same change.

1. **Spikes R2 and R4.** Verify: each result goes into this file as a row in the spike table. No code lands.

   Status: partly done. R2 and R4 are in the risk table: what the app does is built and tested, and the rest needs a person (see "Still open").
2. **Request boundaries in the core.** `begin_request` and `end_request` on `Project` and `Session`. Verify:
   - Tests in `crates/core/tests/project/undo_grouping.rs`: writes 30 s apart in one request make one step with the label; a window edit mid-request makes two steps; R5's late write; the 15 s rule after the end.
   - ARCHITECTURE.md, "The live folder and editing", is updated.

   Status: done, with those tests and that section.
3. **`sound-agent` with the Claude driver.** The provider seam, the Claude protocol types, `AgentEvent`, and a dev example (`cargo run -p sound-agent --example chat -- <folder>`) that chats in the terminal with a `claude` found on PATH. Verify:
   - Fixture tests: recorded JSONL of real runs (plain answer, file edit, approval, denied tool, interrupt, permission mode change, stale resume, process crash), with `insta` snapshots of the `AgentEvent`s.
   - One `#[ignore]` live test in a temp folder.
   - The trimmed flags still load the project's `CLAUDE.md`: the live test asks for a fact that is only in `AGENTS.md`.

   Status: done. Fixtures in `crates/agent/tests/fixtures/claude/` with snapshots in `provider/claude/tests.rs`, the driver against a fake `claude` in `process_tests.rs`, and ignored live tests in `crates/agent/tests/live.rs` (the `AGENTS.md` fact and a resume, settings in the project ignored, each approval mode).
4. **Sidebar end to end, plain text.** The left panel slot, the title-row icon, cmd-L, the remembered open-or-closed flag, the sidebar, `list`, the current single-line composer, send, stop and request boundaries, still using a `claude` found on PATH. Verify:
   - A window test in `crates/runtime/tests/window/` feeds recorded events and checks the thread, the undo label and that it is one undo step.
   - A window test with a temporary support folder: close the sidebar, reopen the window, and check it is still closed. cmd-L opens it with the composer focused.
   - Window snapshots at 1470 x 920 with the sidebar open, and closed with the working indicator on the icon.
   - A manual run: open a project, ask "Add a bass line in bars 5 to 8 that follows the piano", see the clips appear, and check that one cmd-z removes all of it.

   Status: done. Window tests in `crates/runtime/tests/window/agent.rs` (`a_request_of_the_agent_is_one_undo_step_named_after_the_message`, `the_panel_stays_closed_once_closed_and_cmd_l_opens_it_on_the_composer`, `a_closed_panel_shows_the_agent_working_on_its_icon`), snapshots `agent-sidebar.png` and `agent-closed-working.png`. In place of the manual run, an ignored test with the real `claude` (`the_agent_adds_a_clip_as_one_undo_step` in `crates/runtime/tests/projects/agent.rs`) asks for a clip and checks that one undo takes it back.
5. **Install and sign-in.** The pinned download, the onboarding states and both sign-in choices. Verify:
   - Unit tests of the install state machine against a local file: checksum mismatch, resume, removing an old version.
   - Gallery states for every row of the table in section 2.
   - A manual run as a new macOS user with no `claude`: set up, sign in and send a message, with no terminal opened.

   Status: done but for the manual run. Install tests in `crates/agent/src/install/tests.rs`, the onboarding section of the gallery, and ignored tests in `crates/agent/tests/setup.rs` that download the pinned `claude` and start its sign-in. The manual run is R2, still open.
6. **Chat quality.** Markdown, the multi-line composer with history, steps and "Worked for", approvals and the approval setting, the problems line, and the model picker. Verify:
   - A unit test that each `ApprovalMode` maps to the permission mode in section 5.
   - Unit tests of the markdown block model and of the composer's wrap and cursor math. Both are pure functions.
   - Gallery snapshots of the composer (empty, 3 lines, full) and of the sidebar (working, done with steps open, failed, approval, problems, long thread).
   - A manual run of each setting: "Ask for everything" asks before an edit, "Ask before commands" edits and asks before `cargo build`, and "Never ask" asks nothing.

   Status: done. Settings tests in `crates/agent/src/settings.rs`, history in `view/history.rs`, the mode mapping in the driver's flag test, `Markdown::inline_code` for titles. Window tests: the problems line, up and down in the composer, a write heard just after a turn ended, the menu's settings reaching the agent at once, also mid-turn, two sidebars sharing one setting, and up on a thread opened again. Window snapshots `agent-*.png` (see `crates/runtime/tests/snapshots/agent.rs`). In place of the manual run, an ignored live test (`each_approval_mode_asks_as_it_says`) with haiku: "Ask for everything" asked before the Write and `git init`, "Ask before commands" only before `git init`, "Never ask" asked nothing, and `ls` asked in no mode. The menu shows the models once the first agent of the app started; until then only the picked one, and its button says "Default" or the picked id.
7. **Threads saved and resumed.** The store, resume, **+** and the stale-resume message. Verify:
   - Store tests: a round trip; a damaged line is skipped with a notice; two projects never share a thread.
   - A manual run: quit mid-thread, reopen, and ask "what did we just change?"

   Status: done. Store tests in `crates/agent/src/store/tests.rs`; window tests close and reopen a project with the same support folder (`a_thread_opens_again_as_it_was_and_resumes_its_session`, `a_lost_session_ends_the_thread_until_plus`); snapshot `agent-cannot-continue.png`. In place of the manual run, an ignored window test with the real `claude` (`the_real_agent_resumes_a_thread_after_the_window_closed`) says "remember the word lantern", closes the window, opens the project again and gets "lantern" back. An `index.json` that does not read is kept as `index.json.bad` with a notice, never written over.
8. **Docs and release check.** ARCHITECTURE.md gets a section on the agent sidebar, and its "Agent context" section changes. DESIGN.md gets the new keys and the changed rule on saved interface state. CONCEPT.md "Where it stands" changes. Verify:
   - The Linux CI job builds and passes the tests. The sidebar compiles there, and the pinned download covers Linux.
   - A manual run of the `.app` on a second Mac.

   Status: the docs are done, and CI passes on Linux. The run on a second Mac is still open.

### Still open

These need a person and a real Mac:

- R2: finish a real sign-in to the end, in a throwaway `CLAUDE_CONFIG_DIR`, and check `auth status` after.
- R4: launch the `.app` from the Finder and check that the agent finds `cargo`.
- R7: frame times while a long answer streams in the app. The parse time is measured.
- Milestone 8: run the `.app` on a second Mac.

## Open issues

- With the sidebar open, the window can still be made as narrow as `MIN_WINDOW_WIDTH` (1100 pt), which leaves the arrangement 740 pt. The minimum size does not grow with the panel.
- Under "Ask before commands" the agent still asks before a compound command that only reads, such as a loop over `$(find …)` or `find -exec` (milestone 6 spike). Plain reads do not ask.
- **Sign out** does nothing visible when `ANTHROPIC_API_KEY` is set in the login shell: Claude Code then counts as signed in with that key, so the sidebar comes back ready.

## Adding Codex later

Not in v1. These notes save the next agent the research. Adding Codex is a `Provider::Codex` variant and a private `provider/codex.rs`. The thread, view, install and store code stay as they are.

- **Spiked on codex-cli 0.159.2.** Driving `codex app-server` over stdio JSON-RPC (`initialize`, `initialized`, `account/read`, `thread/start` with `cwd`, `sandbox` and `approvalPolicy`, then `turn/start`) ran a real turn that wrote a file. The turn ends with `turn/completed`, and the server exits when stdin closes.
- **Protocol.** `codex app-server generate-json-schema` lists 104 client requests, 83 notifications and 10 server requests. We would need about 15 of them. Write our own serde types, checked against the schema of the pinned binary. Rejected: `codex-app-server-protocol` as a git dependency, because it pulls in about 10 crates of the Codex workspace. Do not enable `experimentalApi`.
- **Approval mapping** (from the hooman runner):
  - Ask for everything: `untrusted` / `read-only`.
  - Ask before commands: `on-request` / `workspace-write`.
  - Never ask: `never` / `danger-full-access`.

  Approvals come as the server requests `item/commandExecution/requestApproval` and `item/fileChange/requestApproval`, answered with `accept`, `acceptForSession` or `decline`. `turn/start` takes a new approval policy.
- **Trim.** By default Codex loads the user's MCP servers, as the spike showed. Find the `-c` overrides that turn them off.
- **Install.** The asset is `codex-<target>.tar.gz` (91 MB) from the GitHub release `rust-v<version>`, which lists a sha256. Apache-2.0 allows the download; keep the NOTICE. The standalone `codex-app-server` asset (69 MB) may be enough, which needs a spike to check that sandboxed commands work.
- **Sign-in.** `account/read` checks the account. The two choices:
  - `account/login/start {type: chatgpt}` returns an `authUrl` to open, then wait for `account/login/completed`. Callback port 1455; check it while the Codex app is open.
  - `{type: apiKey}` passes a key once, and Codex stores it.

  Credentials live in `~/.codex/auth.json` or the keyring.
- **Terms.** App-server sign-in is allowed for "a local or open-source application" and "has never been permitted for commercial or hosted services". Sound Tools stays open source, so it is allowed. A paid version would need "Sign in with ChatGPT" (partner waitlist).
- **Threads.** Resume with `thread/resume` and the thread id. A stale thread says "no rollout found" or "thread not found".
- **hooman reference.** `src/codex/` (`rpc.ts`, `protocol.ts`, `codex.ts`, `codex-events.ts`) holds the gotchas:
  - Codex has no request deadlines of its own.
  - Stdout occasionally carries stray non-JSON lines.
  - `status` is the only reliable field on an item.

## Sources

- Claude Code legal and compliance, checked 2026-09-30: https://code.claude.com/docs/en/legal-and-compliance
- Claude Agent SDK overview: https://code.claude.com/docs/en/agent-sdk/overview
- Claude headless mode and CLI reference: https://code.claude.com/docs/en/headless, https://code.claude.com/docs/en/cli-reference
- Codex app-server, section "Auth endpoints": https://learn.chatgpt.com/docs/app-server
- Codex releases: https://github.com/openai/codex/releases/tag/rust-v0.159.2
- Zed external agents: https://zed.dev/docs/ai/external-agents
- T3 Code: https://github.com/pingdotgg/t3code
- Conductor FAQ: https://www.conductor.build/docs/faq
- ACP registry: https://agentclientprotocol.com/get-started/registry
- hooman runner: `~/hooman/hooman-studio/apps/runner/src` (`claude/`, `codex/`, `session.ts`, `threads.ts`, `shell-env.ts`)
- gpui `list`: `crates/gpui/src/elements/list.rs` at Zed rev `7c451e69`
