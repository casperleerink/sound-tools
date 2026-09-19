//! Rules about the workspace itself, checked by tests.
//!
//! Extensions live in `extensions/`. They depend on the core, the UI SDK and small shared
//! contract crates, never on each other. This keeps the build wide and parallel, which
//! protects the extension edit loop.

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

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
    fn finds_direct_and_transitive_extension_dependencies() {
        let id = |name: &str| PackageId {
            repr: name.to_string(),
        };
        let (synth, arrangement, mixer, contract, helper) = (
            id("synth"),
            id("arrangement"),
            id("mixer"),
            id("contract"),
            id("helper"),
        );
        let extensions = HashSet::from([&synth, &arrangement, &mixer]);
        let arrangement_dependencies = [contract.clone(), helper.clone()];
        let synth_dependencies = [contract];
        let helper_dependencies = [mixer.clone()];
        let dependencies = HashMap::from([
            (&arrangement, arrangement_dependencies.as_slice()),
            (&synth, synth_dependencies.as_slice()),
            (&helper, helper_dependencies.as_slice()),
        ]);

        let violations = extension_dependency_violations(&extensions, &dependencies);
        assert_eq!(violations, vec![(&arrangement, &mixer)]);
    }
}
