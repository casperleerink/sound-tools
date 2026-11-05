// Pi loads this file with `-e`. Its questions reach Sound Tools as the extension UI requests
// of RPC mode: a `select` per approval, an `input` per sign-in question, and a `notify` of
// JSON for everything Sound Tools reads.
import path from "node:path";
import { ModelRuntime, SettingsManager } from "@earendil-works/pi-coding-agent";

const READS = new Set(["read", "grep", "find", "ls"]);
const CHANGES = new Set(["edit", "write"]);

// Whether the tool call stays in the project: its path, or the project when it names none.
// Pi's tools take any path, so one outside the project always asks, as Claude Code does.
function inProject(event, cwd) {
  const target = path.resolve(cwd, event.input.path ?? ".");
  const relative = path.relative(cwd, target);
  return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
}

export default function (pi) {
  const say = (ctx, record) => ctx.ui.notify(JSON.stringify(record));

  pi.registerFlag("sound-tools-approval", { type: "string", default: "ask_before_commands" });
  let mode;
  // What the composer allowed for the thread: a tool by name, or one command whole.
  const allowed = new Set();

  pi.registerCommand("sound-tools-approval", {
    handler: async (args) => {
      mode = args.trim();
    },
  });

  pi.on("tool_call", async (event, ctx) => {
    mode ??= pi.getFlag("sound-tools-approval");
    const tool = event.toolName;
    const key = tool === "bash" ? `bash:${event.input.command}` : tool;
    if (mode === "never_ask" || allowed.has(key)) return undefined;
    const inside = inProject(event, process.cwd());
    if (READS.has(tool) && inside) return undefined;
    if (mode === "ask_before_commands" && CHANGES.has(tool) && inside) return undefined;
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
}
