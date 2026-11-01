/// What the runtime writes into a project when it opens: hidden files, the agent docs and the
/// SDK beside the extensions. A template holds none of them, and a project that holds nothing
/// else is still empty.
pub(crate) fn written_by_the_runtime(path: &str) -> bool {
    path.split('/')
        .any(|part| part.starts_with('.') || part == "node_modules")
        || ["AGENTS.md", "CLAUDE.md", "problems.txt"].contains(&path)
        || path.starts_with("agent-docs/")
        || path.starts_with("extensions/sdk/")
        || [
            "extensions/sdk.ts",
            "extensions/tsconfig.json",
            "extensions/bun.d.ts",
            "extensions/log.txt",
        ]
        .contains(&path)
}
