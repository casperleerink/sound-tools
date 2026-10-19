//! Rules about the workspace itself, checked by tests.
//!
//! Extensions live in `extensions/`. They depend on the core, the UI SDK and small shared
//! contract crates, never on each other. This keeps the build wide and parallel, which
//! protects the extension edit loop.
//!
//! The core depends on no other crate of the workspace, so it knows no music.
//!
//! The top-level docs stay within a word limit, and code never points at them.

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::path::{Path, PathBuf};

    use cargo_metadata::{MetadataCommand, PackageId};

    /// Pairs of (extension, other extension it reaches through its dependencies).
    fn extension_dependency_violations<'a>(
        extensions: &HashSet<&'a PackageId>,
        dependencies: &HashMap<&'a PackageId, &'a [PackageId]>,
    ) -> Vec<(&'a PackageId, &'a PackageId)> {
        let mut violations = Vec::new();
        for &extension in extensions {
            let mut visited = HashSet::from([extension]);
            let mut pending = vec![extension];
            while let Some(package) = pending.pop() {
                for dependency in dependencies.get(package).copied().unwrap_or_default() {
                    if !visited.insert(dependency) {
                        continue;
                    }
                    if extensions.contains(dependency) {
                        violations.push((extension, dependency));
                    }
                    pending.push(dependency);
                }
            }
        }
        violations
    }

    #[test]
    fn extensions_do_not_depend_on_each_other() -> anyhow::Result<()> {
        let metadata = MetadataCommand::new().exec()?;
        let extensions_folder = metadata.workspace_root.join("extensions");
        let extensions = metadata
            .workspace_packages()
            .into_iter()
            .filter(|package| package.manifest_path.starts_with(&extensions_folder))
            .map(|package| &package.id)
            .collect();
        let resolve = metadata
            .resolve
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("cargo metadata returned no dependency graph"))?;
        let dependencies = resolve
            .nodes
            .iter()
            .map(|node| (&node.id, node.dependencies.as_slice()))
            .collect();

        let violations = extension_dependency_violations(&extensions, &dependencies);
        assert!(
            violations.is_empty(),
            "extensions must not depend on each other: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn core_depends_on_no_workspace_crate() -> anyhow::Result<()> {
        let metadata = MetadataCommand::new().exec()?;
        let workspace: HashSet<&str> = metadata
            .workspace_packages()
            .into_iter()
            .map(|package| package.name.as_str())
            .collect();
        let core = metadata
            .workspace_packages()
            .into_iter()
            .find(|package| package.name.as_str() == "sound-core")
            .ok_or_else(|| anyhow::anyhow!("the workspace has no sound-core"))?;
        let violations: Vec<&str> = core
            .dependencies
            .iter()
            .map(|dependency| dependency.name.as_str())
            .filter(|name| workspace.contains(name))
            .collect();
        assert!(
            violations.is_empty(),
            "the core must not depend on a workspace crate: {violations:?}"
        );
        Ok(())
    }

    /// The top-level docs and their word limits.
    const DOCS: [(&str, usize); 4] = [
        ("ARCHITECTURE", 5_000),
        ("DESIGN", 1_800),
        ("ENGINEERING", 2_000),
        ("CONCEPT", 500),
    ];

    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// Docs hold decisions, not what the code already says. Each time they grew past what an
    /// agent should read, most of the growth was code retold. A limit makes a new line push
    /// out an old one.
    #[test]
    fn docs_stay_within_their_word_limits() -> anyhow::Result<()> {
        let mut over = Vec::new();
        for (doc, limit) in DOCS {
            let words = fs::read_to_string(workspace_root().join(format!("{doc}.md")))?
                .split_whitespace()
                .count();
            if words > limit {
                over.push(format!("{doc}.md: {words} words, limit {limit}"));
            }
        }
        assert!(
            over.is_empty(),
            "cut these docs to decisions, see AGENTS.md: {over:?}"
        );
        Ok(())
    }

    /// Every file under `folders`, outside build output and hidden folders.
    fn files(folders: &[&str]) -> anyhow::Result<Vec<PathBuf>> {
        let mut pending: Vec<PathBuf> = folders
            .iter()
            .map(|folder| workspace_root().join(folder))
            .collect();
        let mut files = Vec::new();
        while let Some(path) = pending.pop() {
            if !path.is_dir() {
                files.push(path);
                continue;
            }
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name == "target" || name == "node_modules" || name.starts_with('.') {
                continue;
            }
            for entry in fs::read_dir(&path)? {
                pending.push(entry?.path());
            }
        }
        Ok(files)
    }

    /// Code that names a doc section breaks silently when the doc is cut or renamed. Docs
    /// point at code instead.
    #[test]
    fn code_does_not_point_at_docs() -> anyhow::Result<()> {
        // Named without the extension here, so this file does not match itself.
        let docs = DOCS.map(|(doc, _)| format!("{doc}.md"));
        let mut violations = Vec::new();
        for path in files(&["crates", "extensions", "tooling", "Cargo.toml"])? {
            if path.extension().is_some_and(|extension| extension == "md") {
                continue;
            }
            // Snapshots and other binary files are not code.
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if docs.iter().any(|doc| text.contains(doc.as_str())) {
                violations.push(path);
            }
        }
        assert!(
            violations.is_empty(),
            "code must not point at the top-level docs: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn finds_direct_and_transitive_extension_dependencies() {
        let id = |name: &str| PackageId {
            repr: name.to_string(),
        };
        let (synth, song, mixer, contract, helper) = (
            id("synth"),
            id("song"),
            id("mixer"),
            id("contract"),
            id("helper"),
        );
        let extensions = HashSet::from([&synth, &song, &mixer]);
        let song_dependencies = [contract.clone(), helper.clone()];
        let synth_dependencies = [contract];
        let helper_dependencies = [mixer.clone()];
        let dependencies = HashMap::from([
            (&song, song_dependencies.as_slice()),
            (&synth, synth_dependencies.as_slice()),
            (&helper, helper_dependencies.as_slice()),
        ]);

        let violations = extension_dependency_violations(&extensions, &dependencies);
        assert_eq!(violations, vec![(&song, &mixer)]);
    }
}
