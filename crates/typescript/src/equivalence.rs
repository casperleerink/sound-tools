//! Temporary, until Hum is gone: the graph a tool sends now lowers to exactly the operations
//! the Hum of the SDK before it compiled to, for every tool of the agent evals, of the doc for
//! agents and of the tests, with every combination of its choices.

use std::path::Path;

use serde_json::{Map, Value};

use crate::bun::{Bun, Request};
use crate::tools::{Choice, Field, ToolInfo};

/// The commit whose `sdk.ts` and `host.ts` wrote Hum.
const BEFORE: &str = "1a1c1e2";

fn before(path: &str) -> String {
    let output = std::process::Command::new("git")
        .args(["show", &format!("{BEFORE}:{path}")])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(output.status.success(), "git show {BEFORE}:{path}");
    String::from_utf8(output.stdout).unwrap()
}

/// Every tool file to compare, by a file name.
fn sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut sources = Vec::new();
    for folder in ["tooling/agent-eval/examples", "tooling/agent-eval/fixtures"] {
        let mut entries: Vec<_> = (std::fs::read_dir(root.join(folder)).unwrap())
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            sources.push((name, std::fs::read_to_string(&path).unwrap()));
        }
    }
    let doc = include_str!("agent-doc.md");
    for (index, block) in doc.split("```").enumerate().skip(1).step_by(2) {
        let (language, code) = block.split_once('\n').unwrap();
        if code.contains("tool({") && code.contains("sound:") {
            sources.push((format!("doc{index}.{language}"), code.to_string()));
        }
    }
    for test in [
        "crates/runtime/tests/projects/typescript.rs",
        "crates/runtime/tests/window/tools.rs",
    ] {
        let text = std::fs::read_to_string(root.join(test)).unwrap();
        for (index, rest) in text.split("r#\"import").enumerate().skip(1) {
            let code = format!("import{}", rest.split("\"#").next().unwrap());
            // A tool written with `format!`.
            let code = match code.contains("{{") {
                true => (code.replace("{{", "{").replace("}}", "}")).replace("{level}", "0.5"),
                false => code,
            };
            let name = Path::new(test).file_stem().unwrap().to_string_lossy();
            sources.push((format!("{name}{index}.ts"), code));
        }
    }
    sources
}

/// Bun on `file` alone, with `sdk` and `host`.
fn start(
    sdk: &str,
    host: &str,
    file: &(String, String),
) -> (Bun, Vec<ToolInfo>, [tempfile::TempDir; 2]) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("sdk.ts"), sdk).unwrap();
    std::fs::write(folder.path().join(&file.0), &file.1).unwrap();
    let host_folder = tempfile::tempdir().unwrap();
    let host_path = host_folder.path().join("host.ts");
    std::fs::write(&host_path, host).unwrap();
    let program = crate::bun_program().unwrap();
    let (bun, loaded) = Bun::start(&program, &host_path, folder.path()).unwrap();
    let errors: Vec<&str> = (loaded.errors.iter())
        .map(|error| error.message.as_str())
        .collect();
    assert!(errors.is_empty(), "{}: {errors:?}", file.0);
    (bun, loaded.tools, [folder, host_folder])
}

/// Every combination of the choices of `info`.
fn combinations(info: &ToolInfo) -> Vec<Map<String, Value>> {
    let mut all = vec![Map::new()];
    for (name, field) in &info.fields.0 {
        if let Field::Choice { options, .. } = field {
            all = (all.iter())
                .flat_map(|choices| {
                    options.iter().map(move |option: &Choice| {
                        let mut choices = choices.clone();
                        choices.insert(name.clone(), option.to_value());
                        choices
                    })
                })
                .collect();
        }
    }
    all
}

#[test]
fn every_tool_lowers_to_the_operations_its_hum_compiled_to() {
    if !crate::has_bun() {
        eprintln!("skipped: Bun is not installed");
        return;
    }
    let (old_sdk, old_host) = (
        before("crates/typescript/src/sdk.ts"),
        before("crates/typescript/src/host.ts"),
    );
    let mut compared = 0;
    for file in sources() {
        let (old, _, _old_folder) = start(&old_sdk, &old_host, &file);
        let (new, tools, _new_folder) = start(crate::SDK, crate::HOST, &file);
        assert!(!tools.is_empty(), "{} defines no tool", file.0);
        for info in &tools {
            for choices in combinations(info) {
                let tool = info.name.as_str();
                let choices = &choices;
                let lines = old.ask(|id| Request::Sound { id, tool, choices }).unwrap();
                let lines: Vec<String> = serde_json::from_value(lines).unwrap();
                let mut hum = sound_hum::compile(&lines).unwrap();
                let graph = new.sound(tool, choices).unwrap();
                let mut lowered = graph.compile(&info.declarations()).unwrap();
                (hum.hash, lowered.hash) = (0, 0);
                assert_eq!(
                    format!("{lowered:?}"),
                    format!("{hum:?}"),
                    "{} {tool} {choices:?}",
                    file.0
                );
                compared += 1;
                eprintln!(
                    "{}: {tool} {choices:?}: {} lines of Hum",
                    file.0,
                    lines.len()
                );
            }
        }
    }
    eprintln!("{compared} sounds compared");
    assert!(compared > 20, "{compared}");
}
