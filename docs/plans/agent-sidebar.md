# Plan: the agent sidebar

## Summary for Casper

- **Approach.** Rust talks to the two CLIs directly: `claude` in stream-json mode and `codex app-server` over JSON-RPC. No Node and no sidecar. Spikes ran a real turn, an approval and a resume on both, each in about 60 lines of script.
- **Install.** On first use the app downloads its own pinned, checksum-checked copy of the chosen CLI into Application Support (Codex 91 MB, Claude 215 MB). Nothing is bundled and no terminal is needed.
- **Sign in.** Each CLI's own sign-in flow opens the browser and returns by itself. The app never sees or stores a token. A Mac that is already signed in just works.
- **UI.** gpui's `list` for the thread, a small markdown renderer of our own, and our `TextInput` made multi-line. We port nothing: Zed's agent code is GPL, and the gpui-component pieces are over 20k lines each.
- **Undo.** The sidebar says where each request begins and ends, so one request is one undo step. The 15 s guess stays only for agents in a terminal.
- **v1.** Both providers, one thread per project that resumes on reopen, tool steps folded behind "Worked for 12 s", no diffs, no attachments.
- **Trade-off.** We own two protocol drivers. Claude's control messages are not documented, so we pin the CLI version and test against recorded transcripts.
- **Decided.** Approvals are a user setting, with "Ask before commands" as the default. The sidebar collapses, and the app remembers whether it was open. Sound Tools stays open source, so the CLIs' own sign-in is allowed.

---

## Goal

An agent sidebar on the left of the window. The composer talks to Claude Code or Codex there, and the agent works in the open project folder as the terminal agents do today. A composer who has never opened a terminal can install, sign in and ask for a change.

## Decided constraints

- Providers are Claude Code and Codex. Adding a provider later is a new variant plus a driver module, and nothing else changes.
- The agent's cwd is the project folder. It edits files and the runtime applies them live. There is no new edit API.
- The composer never needs a terminal. The app installs the CLIs and runs each provider's own sign-in (subscription or API key).
- `sound-core` and `sound-ui` never depend on the agent code (ARCHITECTURE.md, "Direction"). Anything the sidebar needs from them, such as request boundaries, is generic and usable by any agent.
- UI follows DESIGN.md: the quiet rule, dark only, lavender means the agent, nothing blocks. Components come from `crates/ui`. No gpui-component.
- Chat and interface state stay out of the project folder. The folder is only what the agent writes.
- Credentials stay in each CLI's own store. The app never reads, stores or forwards tokens.
- The hooman-studio runner (`~/hooman/hooman-studio/apps/runner`) is the reference for behaviour and edge cases. We port its logic to Rust and do not run it.
- How much the agent may do without asking is a user setting (section 5).
- The sidebar collapses. The app remembers whether it is open, per machine and not per project (section 6).
- Sound Tools stays open source and local. That is what makes the Codex app-server sign-in allowed (section 2).
- The two remembered settings live in the machine's support folder, never in a project. An agent that works in the project folder must not be able to give itself more access by editing a file there.

## What the spikes showed

Throwaway Python scripts in `/tmp`, run against the CLIs on this Mac (claude 2.1.286, codex-cli 0.159.2). None of them is committed.

| Spike | Result |
| --- | --- |
| `codex app-server`: `initialize`, `account/read`, `thread/start` (cwd, `workspace-write`, `on-request`), `turn/start` | Works. The agent wrote a file in the folder. The turn ends with an explicit `turn/completed`. `account/read` gives the account and plan. |
| `codex app-server generate-json-schema` | 104 client requests, 83 notifications, 10 server requests. We need about 15 of them. |
| `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --permission-prompt-tool stdio` | Works. The `initialize` control request returns `account` (email, plan) and `models`. A Write triggers `control_request can_use_tool`, and our `allow` reply lets it through. The turn ends with a `result` message. |
| `claude --resume <session-id>` in stream-json | Works. The agent remembered the earlier turn. |
| `claude auth status --json` | Returns `loggedIn`, `authMethod` and `subscriptionType`. Exit code 1 when signed out. |
| `claude auth login` with no terminal, in a throwaway `CLAUDE_CONFIG_DIR` | Opens the browser and listens on a localhost port for the callback, so sign-in completes without pasting a code. It also prints a fallback URL and a "paste code" prompt. A real sign-in to the end was not done (see risks). |
| Binary sizes | Claude native binary 215 MB. Codex `codex` tar.gz 91 MB, standalone `codex-app-server` 69 MB. Each release lists a sha256. |
| Claude download | `install.sh` fetches `https://downloads.claude.ai/claude-code-releases/<version>/<platform>/claude` and checks the sha256 in `manifest.json`. We can do the same, unmodified. |
| How others install | The Claude desktop app downloads Claude Code into `Application Support/Claude/claude-code/<version>`. Zed downloads its own Node and the ACP adapters into its data folder. T3 Code uses the CLIs on PATH and asks the user to install them. Conductor bundles both and reuses the Mac's login. |

## 1. Process model

**Recommendation: Rust speaks to each CLI directly.** The drivers are two small modules in one crate. The spikes show that each protocol is newline-delimited JSON over stdio, with an explicit end of turn and approval requests we answer.

| | Rust direct (chosen) | Node/Bun sidecar with the TS Agent SDK | ACP (`agent-client-protocol` 2.2.0 plus adapters) |
| --- | --- | --- | --- |
| Bundle size | +0. CLIs downloaded on use | +60 MB for a compiled Bun sidecar, and the SDK still runs the 225 MB native `claude` | Needs Node plus npm adapters for both Claude and Codex |
| Node on the user's machine | No | Bundled or required | Bundled (as Zed does) |
| Crash isolation | The CLIs are child processes, so a CLI crash is a failed turn. Our driver code follows the no-panic rules | The same, plus one more process | The same, plus adapters |
| hooman code reuse | Port the logic of about 2.5k portable lines (drivers, event mapping, approvals, gotchas) | Lift the TS files almost as they are, then write a third protocol between Rust and the sidecar | None |
| Types | serde enums, checked by the compiler | Types stop at the process edge | Types from the crate, but provider details hide in `_meta` |

- Rejected: **sidecar.** It adds a runtime and our own IPC protocol, and it saves only the mapping code, which is where the Rust types help most.
- Rejected: **ACP now.** Both adapters are npm-only (`@agentclientprotocol/claude-agent-acp` 0.84.0, `codex-acp` 2.0.1), they lag features by weeks, and sign-in is still per agent. ACP is the right way to add Gemini CLI and others later, as one more `Provider` variant.
- Rejected: **`codex-app-server-protocol` as a git dependency.** It pulls in about 10 crates of the Codex workspace. We write serde types for the messages we use, checked against the schema of the pinned binary.
- Rejected: **the unofficial `claude-agent-sdk` Rust crate.** It is maintained by a private person and wraps the same undocumented messages.

How it runs (for the implementer):

- One child process per open thread. `smol::process::Command` with `kill_on_drop` (clippy already requires smol). The stdout reader runs on the background executor, parses each line into a typed message, and hands a batch to the sidebar entity once per frame.
- Every protocol enum has an `Unknown` catch-all (`#[serde(other)]` or an untagged fallback), so a new message kind from the CLI is ignored and logged, not a crash.
- Both CLIs exit when stdin closes, so quitting the app or "Open project…" also ends them.
- The environment comes from the login shell (`$SHELL -ilc 'env -0'`, captured once in the background at start), as hooman's `shell-env.ts` and Zed do. An app opened from the Finder has a bare PATH, and the agent needs `cargo` and `git` to build extensions. Remove `CLAUDECODE`, `CLAUDE_CODE_*` and `ELECTRON_RUN_AS_NODE` (hooman: a nested session never saves its transcript).
- Claude flags: `-p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --permission-prompt-tool stdio --permission-mode <mode> [--resume <id> | --session-id <uuid>] --model <m> --disallowedTools AskUserQuestion`. Pass every flag explicitly, because the docs say `-p` defaults will change (`--bare`). Check the `capabilities` list of `system/init` rather than version strings.
- Codex: `initialize` then `initialized`, then `thread/start` or `thread/resume` with `cwd`, `sandbox` and `approvalPolicy`, then `turn/start`, `turn/interrupt`, `model/list` and the `account/*` methods. Do not enable `experimentalApi`.
- Each provider uses its default home (`~/.claude`, `~/.codex`), so a login made in a terminal or in the vendors' apps is shared. The composer's own Claude and Codex settings and MCP servers load, as they do in a terminal today.

## 2. Install and sign in

**Recommendation: the app keeps its own pinned copy of each CLI and runs the CLI's own sign-in.**

Install:

- The version and sha256 per platform (macOS arm64 and x64, Linux x64 and arm64) are constants in the agent crate. Moving a pin means changing the constant and recording the fixtures again, in the same commit.
- The download goes into `support_folder()/agents/<provider>/<version>/`. Use `/usr/bin/curl` (with `-C -` so a broken download resumes) and `tar`, then check the sha256 with the `sha2` crate. That needs no HTTP or TLS dependency. A folder of an older version is removed after the new one checks out.
  - Codex: the `codex-<target>.tar.gz` asset from the GitHub release `rust-v<version>`. Apache-2.0 allows this; keep the NOTICE. If spike R8 shows the 69 MB `codex-app-server` asset is enough, use it.
  - Claude: `downloads.claude.ai/claude-code-releases/<version>/<platform>/claude`, unmodified, with `DISABLE_AUTOUPDATER=1` in its environment so it stays on the tested version. This is what the Claude desktop app does.
- A download happens only when the composer picks that provider. A composer who uses only Codex never downloads Claude.

Sign in:

- Claude: `claude auth status --json` tells whether the Mac is signed in. The app offers two choices, both of which run Anthropic's own flow in the browser:
  - "Sign in with your Claude plan" runs `claude auth login --claudeai`.
  - "Use an Anthropic Console account (API)" runs `claude auth login --console`.

  The app waits for the child to exit and then checks the status again. It has no key field, so it never handles a Claude credential.
- Codex: `account/read`. If there is no account, the app offers:
  - "Sign in with ChatGPT" sends `account/login/start {type: chatgpt}`, opens the returned `authUrl`, and waits for `account/login/completed`.
  - "Use an API key" is one field. The key goes once to `account/login/start {type: apiKey}`, and Codex stores it. The app does not keep it.
- Credentials live where the CLI puts them: the Keychain or `~/.claude/.credentials.json` for Claude, and `~/.codex/auth.json` or the keyring for Codex.

Terms (checked 2026-09-30, quotes in Sources):

- Claude Code is proprietary. Running it inside a product requires the Commercial Terms, an unmodified binary, every built-in sign-in method kept, and each user paying under their own account. Third-party apps may not offer their own "Claude login" or touch tokens, but "an end user signing in to the unmodified Claude Code binary with their own Claude subscription" is allowed. Our design follows that line. In the UI we may write "runs Claude Code" in plain text, but we must not name a feature after it.
- Codex app-server sign-in is allowed for "a local or open-source application" and "has never been permitted for commercial or hosted services", which need "Sign in with ChatGPT" (partner waitlist). Sound Tools is MIT and local, and it stays open source, so this sign-in is allowed. If that ever changes, Codex needs "Sign in with ChatGPT", and Claude needs a check with Anthropic sales.

What the composer sees, in the sidebar, with nothing blocking the rest of the app:

| State | Sidebar shows |
| --- | --- |
| No provider chosen | Two rows, "Claude Code by Anthropic" and "Codex by OpenAI", each with one line on what setup does ("Downloads 215 MB, then opens your browser to sign in") and **Set up** |
| Downloading | A progress line with MB and percent, and **Cancel** |
| Download failed | One sentence ("Could not download Codex. Check the internet connection.") and **Try again**. A checksum mismatch says "The download was damaged" and deletes the file. A region block repeats Anthropic's message |
| Signed out | The sign-in choices above |
| Signing in | "Finish signing in in your browser.", **Open the page again**, **Cancel**. After 10 minutes it goes back to signed out |
| Sign-in failed or cancelled | Back to signed out with one line saying why |
| Ready | The thread. The account (email and plan) sits in the provider menu, which also has **Sign out** |
| CLI fails to start | "Codex stopped: <first line of its error output>" and **Try again** |

Rejected:

- **Bundling both CLIs in the app.** That adds 306 MB to every download, including for people who use one or neither, and the Claude binary would need a new app release anyway.
- **Using PATH only (the T3 Code way).** It needs a terminal to install, and the CLIs update daily under us.
- **Running `install.sh`.** It writes `~/.local/bin` and the shell profile, and installs "latest" rather than the version we tested.

## 3. Chat UI in gpui

**Recommendation: gpui `list` for the thread, our own small markdown renderer, and a multi-line `TextInput` in `crates/ui`. We port nothing.**

What gpui v1.20.2 gives us (`crates/gpui/src/elements/list.rs`):

- `ListState` handles items of different heights. It has `splice` for a new entry and `remeasure_items(range)` for a message that grows while streaming; its doc names streaming as the use.
- `FollowMode::Tail` keeps the view at the bottom, stops following when the composer scrolls up, and follows again at the bottom. Zed's agent panel uses `ListAlignment::Top` with `FollowMode::Tail`.
- `uniform_list` measures one row and gives every row that height, so it does not fit messages. It is fine for a thread list later.
- There is no text input element. `crates/ui/src/components/text_input.rs` (790 lines) is a single-line copy of gpui's input example: IME, selection, clipboard.

What we build:

| Piece | How | Size |
| --- | --- | --- |
| Thread list | `list` with Top alignment and `FollowMode::Tail`. Deltas are batched once per frame, then `remeasure_items` runs on the growing item | ~150 lines |
| Markdown | `pulldown-cmark` 0.13.4 turns events into a small block model (paragraph, heading, list, quote, code block) with inline runs (bold, italic, code, link), drawn with `StyledText`. The streaming message is parsed again on each frame's batch. Tables render as monospace text. No syntax colours, no selection in v1 | ~500 lines |
| Steps | One line per tool step ("Edited state/arrangement/bass/verse-a.json", "Ran cargo build"). The newest step is the working line; after the turn they fold behind "Worked for 12 s", as in the gallery mockup | ~150 lines |
| Composer | Extend `TextInput` to multi-line: shape with `shape_text` and a wrap width, move up and down across wrapped lines, grow up to 8 lines and then scroll, paste keeps newlines, simple undo. Enter sends, shift-enter adds a newline, up in an empty composer recalls earlier messages of the thread. It is a general component in `crates/ui` with a gallery entry | ~800 lines |
| Approval row | See 5 | ~120 lines |

No diffs in v1. The change is already live in the arrangement, the composer hears it, and one cmd-z takes the whole request back. The gallery mockup (`crates/gallery/src/composed/sidebar.rs`) already chose "no avatars, no timestamps, no tool rows", and this plan keeps that.

Rejected:

- **Porting from Zed's `agent_ui` or `markdown`.** They are GPL-3.0, and they depend on `editor`, `language` and `workspace`.
- **Porting from gpui-component.** Its input is 28k lines, a code editor on a rope. Its markdown view is 24k lines. Its `MessageScroller` (508 lines) is only a thin wrapper on `list`, so we read it for the "jump to bottom" idea and write ours.

## 4. Threads and state

**Recommendation: the CLIs keep the conversation. The app keeps a small display log per thread in the machine's support folder.**

- The store is `support_folder()/agent/threads/<project key>/`, where the project key is the canonical project path, as the plugin window positions already do. It holds `index.json` (thread id, provider, provider session id, title, updated) and one `<thread id>.jsonl` per thread. Each line of the log is one of our own typed entries: user message, agent text, step, approval and answer, turn outcome.
- A thread is resumed with the saved provider session id. Claude chooses `--session-id <uuid>` at the start and passes `--resume <uuid>` later. Codex uses `thread/resume` with the thread id. The display comes from our log, so reopening needs no provider API and looks the same for both.
- If a resume fails (hooman matches "no conversation found", "no rollout found", "thread not found"), the old log stays visible and read-only with "This thread can't continue. Start a new one."
- A project has many threads in the data model. In v1 the sidebar shows the last thread and **+** starts a new one. The older threads stay in the index for a thread list later. The provider and model are chosen per thread; switching provider starts a new thread.
- One turn runs at a time per project, because every turn writes the same folder and makes one undo step. While a turn runs, the send button becomes stop (`turn/interrupt`, or the `interrupt` control request for Claude), and so does cmd-period.
- A thread's process starts on the first send and ends when a new thread starts, the project closes or the app quits.

Rejected:

- **In the project folder.** The log is machine and interface state, it would be noise in git, and the agent would read its own chat.
- **Only the providers' stores.** Claude's stream-json has no "read past messages" call, so we would have to parse its internal JSONL.
- **SQLite as in hooman.** A JSONL file per thread is enough for one composer.

## 5. Links to the app

**Undo.** Add request boundaries to the core, with no agent in them:

- `Project::begin_request(label)` and `Project::end_request()`, with `Session` wrappers.
- While a request is open, every outside change joins one undo step with that label, however far apart the writes are.
- `end_request` closes the step once the watcher has been quiet for its 100 ms grouping window, so a write that lands just after the turn's end still joins.
- A window edit or an undo during a request splits it, as today (`History::push` clears the grouping).
- The label is the composer's message, cut to about 40 characters, so the menu says "Undo Add a bass line in bars 5 to 8…".
- The 15 s heuristic (`OUTSIDE_UNDO_WINDOW`) stays for outside changes made with no request open, which covers terminal agents.
- Begin is called at send. End is called on Claude's `result`, on Codex's `turn/completed`, and when a process dies.

**Live status.** While working, the sidebar shows the lavender pulsing `Indicator` and the newest step as one line. The arrangement needs nothing new, because the watcher already applies each file as it lands.

**Problems.** The sidebar reads `Project::problems()` when a turn begins and when it ends. If the turn left problems that were not there before, a peach line goes under the result ("2 files are not live") and expands to `path: message` lines. The agent docs already tell the agent to check `problems.txt`, so v1 sends nothing back automatically. The window's corner notice stays as it is.

**Approvals.** How much the agent may do without asking is a user setting with three choices. The mapping is the one the hooman runner already uses.

| Setting | What the composer reads | Claude `--permission-mode` | Codex `approvalPolicy` / `sandbox` |
| --- | --- | --- | --- |
| Ask for everything | "Asks before every edit and command." | `default` | `untrusted` / `read-only` |
| Ask before commands (default) | "Edits the project freely, asks before commands." | `acceptEdits` | `on-request` / `workspace-write` |
| Never ask | "Does anything without asking. Undo and git are your safety net." | `bypassPermissions` | `never` / `danger-full-access` |

- The setting is one `enum ApprovalMode` in the agent crate, saved in `support_folder()/agent/settings.json`. It applies to every project on the machine. It is not in the project, so the agent cannot change its own permissions by editing a project file.
- The setting is in the composer's provider menu (the dropdown of the gallery mockup), next to the model, as one select.
- A change applies from the next message. Codex takes it on `turn/start`. Claude takes it through the `set_permission_mode` control request; if that fails on the pinned version, restart the process with `--resume`. Record a fixture of the change for both providers in milestone 3.
- Under "Never ask", the first message of a thread shows one quiet line above the composer ("The agent does anything without asking"), so the mode is never a surprise.

- A request shows as the last item of the thread: one sentence of what the agent wants to do (for example "Run `cargo build`") and **Allow**, **Allow for this thread** and **Deny**, in lavender.
- The buttons are tab stops with focus rings. Nothing is modal, and music and editing go on.
- The answers map to Claude `allow` / `deny` (with the CLI's `permission_suggestions` scoped to the session for "this thread") and to Codex `accept` / `acceptForSession` / `decline`.
- Claude's `AskUserQuestion` is disabled with `--disallowedTools`, so the agent asks in plain text. Codex `item/tool/requestUserInput` gets empty answers, and the agent then asks in text.
- Rejected: **a modal dialog** (DESIGN.md says nothing blocks). Rejected: **a setting per project or per thread.** One setting per machine is enough, and it keeps approvals out of the folder.

## 6. Scope of v1

In v1:

- Both providers, set up and signed in from the sidebar, each with its model picker (models from Claude's `initialize` and Codex's `model/list`).
- One visible thread per project, resumed on reopen, and **+** for a new one.
- Streaming markdown answers, steps behind "Worked for", stop, inline approvals, the problems line, and one undo step per request.
- A multi-line composer with history.
- The approval setting (section 5).
- A collapsible sidebar:
  - One sidebar icon in the title row, right of the traffic lights, opens and closes it.
  - cmd-L (free today) opens it and focuses the composer. Pressed again in the composer, cmd-L closes it.
  - Escape in the composer gives the focus back to the arrangement.
  - Closed means gone, with no rail, and the arrangement takes the full width.
  - While the sidebar is closed and a turn runs or an approval waits, the icon carries the small lavender `Indicator`, so a closed sidebar never hides a question.
  - The app remembers open or closed for the machine, not the project, in `support_folder()` next to `last-project`. A first start opens it.
  - This is an exception to DESIGN.md's "interface state is not saved". That rule keeps settings out of the project folder, where an agent could change them, and this one stays out of it too. Update the rule's wording in the same change.

Later, in rough order of value:

- A thread list.
- Mentions of the selected clip or track (cheap, since an id is a path).
- Dropping audio files into the composer.
- Copying and selecting message text.
- An effort picker.
- Steering a running turn.
- More providers through ACP.
- "Sign in with ChatGPT" if Sound Tools is ever sold.

Diffs come only if composers ask for them.

## Shape of the code

- New crate `crates/agent` (package `sound-agent`). It depends on `sound-core`, `sound-ui` and gpui; nothing depends on it except the composition root.
  - `provider/` holds `Provider { Claude, Codex }`, with one module per driver, each mapping its protocol to one `AgentEvent` enum: turn started, text delta, text done, step started or done, approval requested, turn ended with an outcome, process exited.
  - Also `install.rs`, `auth.rs`, `thread.rs` (the state built from events, and the store) and `view/` (sidebar, onboarding, message list, markdown, approval row).
- Keep the drivers apart from the thread state. Tests can then feed `AgentEvent`s to the thread and view with no process.
- The window gets a generic left panel slot: a GPUI global holding a constructor from `Entity<Session>` to `AnyView`, filled in `crates/runtime/src/lib.rs` where views are registered. `window.rs` names no agent type.
- The runtime owns the open-or-closed flag and the title-row icon, because they are about the window's generic left panel. The agent crate gives the slot a way to say "working or waiting" for the icon's `Indicator`. The window code still names no agent type.
- The sidebar has a fixed width of 360 pt. When the window is narrower than `MIN_WINDOW_WIDTH` plus 360, opening the sidebar makes the window wider, or leaves it as it is if the screen has no room; the arrangement then scrolls.
- The multi-line `TextInput` and the markdown renderer are general, so they go in `crates/ui` with gallery entries. The sidebar is not general, so it stays in `crates/agent`.

## Risks and the spike that removes each

| # | Risk | Spike |
| --- | --- | --- |
| R1 | Claude's control messages are undocumented and could change | Partly removed (initialize, can_use_tool and resume work on 2.1.286). Still to do: record `interrupt`, a denied tool and an error turn. Pin the version, check `capabilities`, and test against fixtures |
| R2 | `claude auth login` without a terminal might not finish | Partly removed (it opens the browser and listens on localhost). Still to do: complete a real sign-in into a throwaway `CLAUDE_CONFIG_DIR`, check exit code 0 and `auth status`, and check the Keychain item with the default folder |
| R3 | Codex ChatGPT sign-in through app-server | Run `account/login/start {type: chatgpt}` with a throwaway `CODEX_HOME` to the end. Check port 1455 while the Codex app is open |
| R4 | An app opened from the Finder has a bare PATH, so the agent cannot find `cargo` | Launch the `.app` from the Finder and have the agent run `which cargo`, with and without the login-shell env |
| R5 | The agent's last write lands after the end of the turn and misses the undo step | Core test with `apply_outside_changes`: a write 50 ms after `end_request` joins, a write 500 ms after starts a new step |
| R6 | The multi-line composer is the largest UI piece | Gallery spike: wrapping, up and down across wrapped lines, and growth to 8 lines, before the rest |
| R7 | Parsing a long streaming message on every frame is too slow | Stream a 5 KB answer into a 200-message thread and check frame times in a release build |
| R8 | The 69 MB `codex-app-server` asset may not sandbox commands alone | Run the Codex spike script against that asset; use it if a sandboxed command works |
| R9 | Terms change, or Sound Tools stops being open source | Not a spike. Check both terms pages before each release |
| R10 | Claude cannot change its permission mode on a running process | Record a `set_permission_mode` control request on the pinned version in milestone 3. If it fails, restart with `--resume` |

## Milestones

Each milestone ends green on the README checks, with docs updated in the same change.

1. **Spikes R2, R3, R4, R8.** Verify: each result goes into this file (a table row per spike). No code lands.
2. **Request boundaries in the core.** `begin_request` and `end_request` on `Project` and `Session`. Verify: tests in `crates/core/tests/project/undo_grouping.rs` for writes 30 s apart in one request (one step with the label), a window edit mid-request (two steps), R5's late write, and the 15 s rule after the end. Update ARCHITECTURE.md, "The live folder and editing".
3. **`sound-agent` drivers.** Protocol types, both drivers, `AgentEvent`, and a dev example (`cargo run -p sound-agent --example chat -- <folder> claude|codex`) that chats in the terminal with a CLI found on PATH. Verify:
   - Fixture tests: recorded JSONL of real runs (plain answer, file edit, approval, denied tool, interrupt, stale resume, process crash) for each provider, with `insta` snapshots of the `AgentEvent`s.
   - One `#[ignore]` live test per provider in a temp folder.
   - The Codex types deserialize every recorded message, and a test lists the schema methods we rely on.
4. **Sidebar end to end, plain text.** The left panel slot, the title-row icon, cmd-L, the remembered open-or-closed flag, the sidebar, `list`, the single-line composer as it is, send, stop and request boundaries, using a CLI found on PATH. Verify:
   - A window test in `crates/runtime/tests/window/` that feeds recorded events and checks the thread, the undo label and one undo step.
   - A window test with a temporary support folder: close the sidebar, reopen the window, and check it is still closed. cmd-L opens it with the composer focused.
   - Window snapshots at 1470 x 920 with the sidebar open, and closed with the working indicator on the icon.
   - A manual run: open a project, ask "Add a bass line in bars 5 to 8 that follows the piano", see the clips appear, and check that one cmd-z removes all of it.
5. **Install and sign in.** Pinned downloads, the onboarding states, and sign-in for both providers. Verify:
   - Unit tests of the install state machine (checksum mismatch, resume, removing an old version) against a local file.
   - Gallery states for every row of the table in section 2.
   - A manual run as a new macOS user with no CLIs: set up both providers, sign in, and send a message, with no terminal opened.
6. **Chat quality.** Markdown, the multi-line composer with history, steps and "Worked for", approvals and the approval setting, the problems line, and the model picker. Verify:
   - A unit test that each `ApprovalMode` maps to the flags and params in the table of section 5. The mapping is one exhaustive `match`, so a new mode fails to compile until it is mapped.
   - A manual run of each setting with each provider: "Ask for everything" asks before an edit, "Ask before commands" edits and asks before `cargo build`, "Never ask" asks nothing.
   - Unit tests of the markdown block model and of the composer's wrap and cursor math (pure functions).
   - Gallery snapshots for the composer (empty, 3 lines, full) and the sidebar (working, done with steps open, failed, approval, problems, long thread).
   - A manual run of an approval with Claude and with Codex.
7. **Threads saved and resumed.** The store, resume, **+**, and the stale-resume message. Verify:
   - Store tests: round trip, a damaged line is skipped with a notice, two projects never share a thread.
   - A manual run: quit mid-thread, reopen, and ask "what did we just change?"
8. **Docs and release check.** ARCHITECTURE.md gets a section on the agent sidebar, and its "Agent context" section changes. DESIGN.md gets the new keys. CONCEPT.md "Where it stands" changes. Verify: the Linux CI job builds and passes the tests (the sidebar compiles there; the install targets cover Linux), and a manual run of the `.app` on a second Mac.

## Sources

- Claude Code legal and compliance, checked 2026-09-30: https://code.claude.com/docs/en/legal-and-compliance
- Claude Agent SDK overview: https://code.claude.com/docs/en/agent-sdk/overview
- Claude headless and CLI reference: https://code.claude.com/docs/en/headless, https://code.claude.com/docs/en/cli-reference
- Codex app-server, section "Auth endpoints": https://learn.chatgpt.com/docs/app-server
- Sign in with ChatGPT with app-server: https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server
- Codex releases: https://github.com/openai/codex/releases/tag/rust-v0.159.2
- Zed external agents: https://zed.dev/docs/ai/external-agents
- T3 Code: https://github.com/pingdotgg/t3code
- Conductor FAQ: https://www.conductor.build/docs/faq
- ACP registry: https://agentclientprotocol.com/get-started/registry
- hooman runner: `~/hooman/hooman-studio/apps/runner/src` (`claude/`, `codex/`, `session.ts`, `threads.ts`, `shell-env.ts`)
- gpui `list`: `crates/gpui/src/elements/list.rs` at Zed rev `7c451e69`
