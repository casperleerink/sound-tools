// Pi loads this file with `-e`. Its questions reach Sound Tools as the extension UI requests
// of RPC mode: a `select` per approval, an `input` per sign-in question, and a `notify` of
// JSON for everything Sound Tools reads.
import { lstatSync, readlinkSync, realpathSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { ModelRuntime, SettingsManager } from "@earendil-works/pi-coding-agent";

const READS = new Set(["read", "grep", "find", "ls"]);
const CHANGES = new Set(["edit", "write"]);

// Pi reads these as plain spaces in a path.
const UNICODE_SPACES = /[\u00A0\u2000-\u200A\u202F\u205F\u3000]/g;

const WINDOWS = process.platform === "win32";

// A Git Bash path such as `/c/Users` as Windows names it, as Pi reads it there.
function windowsShellPath(given) {
  if (!given.startsWith("/") || given.startsWith("//") || given.includes("\\")) return given;
  const match = given.match(/^\/(?:mnt\/|cygdrive\/)?([a-z])(?:\/(.*))?$/i);
  if (!match) return given;
  return `${match[1].toUpperCase()}:\\${match[2]?.replaceAll("/", "\\") ?? ""}`;
}

// The places a tool's path may name, as Pi's own tools read it: unicode spaces plain, `@`
// dropped, `~` the home folder, a `file://` address its path. For `read` also the names Pi
// tries when the given one is missing, those of macOS screenshots.
function named(tool, input, cwd) {
  let given = input.replace(UNICODE_SPACES, " ");
  if (given.startsWith("@")) given = given.slice(1);
  if (WINDOWS) given = windowsShellPath(given);
  const home = given.startsWith("~/") || (WINDOWS && given.startsWith("~\\"));
  if (given === "~" || home) given = path.join(homedir(), given.slice(2));
  if (given.startsWith("file://")) given = fileURLToPath(given);
  const resolved = path.resolve(cwd, given);
  if (tool !== "read") return [resolved];
  const decomposed = resolved.normalize("NFD");
  const curly = (name) => name.replace(/'/g, "\u2019");
  const screenshot = resolved.replace(/ (AM|PM)\./gi, "\u202F$1.");
  return [resolved, screenshot, decomposed, curly(resolved), curly(decomposed)];
}

// Where a place really is, as the system finds it: one part at a time, each symlink followed
// before the `..` after it, also a symlink to a file that is not there yet, which a write
// would make.
function landing(place) {
  let resolved = path.parse(place).root;
  let parts = place.slice(resolved.length).split(path.sep);
  for (let links = 0; parts.length > 0; ) {
    const part = parts.shift();
    if (part === "" || part === ".") continue;
    if (part === "..") {
      resolved = path.dirname(resolved);
      continue;
    }
    const next = path.join(resolved, part);
    let target;
    try {
      target = lstatSync(next).isSymbolicLink() ? readlinkSync(next) : undefined;
    } catch {
      // Not there: what comes after it is made as named.
    }
    if (target === undefined) {
      resolved = next;
      continue;
    }
    links += 1;
    // A loop of links: the system refuses it too, so whatever it names is not reached.
    if (links > 40) return next;
    if (path.isAbsolute(target)) resolved = path.parse(target).root;
    const rest = target.slice(path.isAbsolute(target) ? resolved.length : 0);
    parts = [...rest.split(WINDOWS ? /[\\/]/ : "/"), ...parts];
  }
  return resolved;
}

// Where the tool call lands outside the project, or `undefined` inside it: its path, or the
// project when it names none. Pi's tools take any path, so one outside always asks, as
// Claude Code does.
function outside(event, cwd) {
  const project = realpathSync(cwd);
  const places = named(event.toolName, event.input.path ?? ".", cwd).map(landing);
  return places.find((place) => {
    const relative = path.relative(project, place);
    return relative !== "" && (relative.startsWith("..") || path.isAbsolute(relative));
  });
}

export default function (pi) {
  const say = (ctx, record) => ctx.ui.notify(JSON.stringify(record));

  pi.registerFlag("sound-tools-approval", { type: "string", default: "ask_before_commands" });
  let mode;
  const allowed = new Set();

  pi.registerCommand("sound-tools-approval", {
    handler: async (args) => {
      mode = args.trim();
    },
  });

  pi.on("tool_call", async (event, ctx) => {
    mode ??= pi.getFlag("sound-tools-approval");
    const tool = event.toolName;
    const landed = tool === "bash" ? undefined : outside(event, process.cwd());
    // Allowed for the thread: one command, one place outside the project, or a tool inside it.
    const key =
      tool === "bash" ? `bash:${event.input.command}` : landed ? `${tool}:${landed}` : tool;
    if (mode === "never_ask" || allowed.has(key)) return undefined;
    if (READS.has(tool) && !landed) return undefined;
    if (mode === "ask_before_commands" && CHANGES.has(tool) && !landed) return undefined;
    // The title is the tool call's id: Sound Tools words the question from its step.
    const answer = await ctx.ui.select(event.toolCallId, ["allow", "allow_for_thread", "deny"], {
      signal: ctx.signal,
    });
    if (answer === "allow_for_thread") allowed.add(key);
    if (answer === "allow" || answer === "allow_for_thread") return undefined;
    return { block: true, reason: "The composer denied this." };
  });

  pi.registerCommand("sound-tools-models", {
    handler: async (_args, ctx) => {
      const models = ctx.modelRegistry.getAvailable().map((model) => ({
        id: `${model.provider}/${model.id}`,
        name: model.name ?? model.id,
        source: ctx.modelRegistry.getProviderDisplayName(model.provider),
      }));
      const current = ctx.model ? `${ctx.model.provider}/${ctx.model.id}` : null;
      say(ctx, { models, current });
    },
  });

  pi.registerCommand("sound-tools-providers", {
    handler: async (_args, ctx) => {
      const runtime = await ModelRuntime.create();
      const providers = runtime
        .getProviders()
        // Pi keeps a provider it replaced under the same sign-in, named "(legacy)".
        .filter((provider) => !provider.name.endsWith("(legacy)"))
        .map((provider) => ({
          id: provider.id,
          name: provider.name,
          sign_in: provider.auth.oauth
            ? (provider.auth.oauth.loginLabel ?? `Sign in with ${provider.auth.oauth.name}`)
            : null,
          api_key: Boolean(provider.auth.apiKey?.login),
          signed_in: runtime.getProviderAuthStatus(provider.id).configured === true,
          // "stored" is what a sign-in here saved, which a sign-out removes.
          stored: runtime.getProviderAuthStatus(provider.id).source === "stored",
        }));
      say(ctx, { providers });
    },
  });

  pi.registerCommand("sound-tools-sign-in", {
    handler: async (args, ctx) => {
      const [id, type] = args.trim().split(/\s+/);
      try {
        const runtime = await ModelRuntime.create();
        const settings = SettingsManager.create(process.cwd());
        await runtime.login(
          id,
          type,
          {
            notify: (step) => say(ctx, { step }),
            prompt: async ({ signal, ...question }) => {
              const answer = await ctx.ui.input(JSON.stringify(question), undefined, { signal });
              if (answer === undefined) throw new Error("Login cancelled");
              return answer;
            },
          },
          { getDeviceId: () => settings.getOrCreateDeviceId() },
        );
        say(ctx, { signed_in: true });
      } catch (error) {
        say(ctx, { failed: error instanceof Error ? error.message : String(error) });
      }
    },
  });

  pi.registerCommand("sound-tools-sign-out", {
    handler: async (args, ctx) => {
      try {
        const runtime = await ModelRuntime.create();
        await runtime.logout(args.trim());
        say(ctx, { signed_out: true });
      } catch (error) {
        say(ctx, { failed: error instanceof Error ? error.message : String(error) });
      }
    },
  });
}
