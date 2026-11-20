//! Which files of a project's code a file imports by a relative path, such as `./song-page`.

/// The paths from the project that the code at `path` may import: for each relative import in
/// `contents`, every file Bun could resolve it to.
pub(super) fn imported(path: &str, contents: &str) -> Vec<String> {
    specifiers(contents)
        .flat_map(|specifier| resolve(path, specifier))
        .collect()
}

/// Whether a file at `path` is code that can import another.
pub(super) fn is_code(path: &str) -> bool {
    [".ts", ".tsx", ".js", ".jsx", ".mts", ".mjs"]
        .iter()
        .any(|end| path.ends_with(end))
}

/// The relative specifiers after `from`, `import` or `import(`.
fn specifiers(contents: &str) -> impl Iterator<Item = &str> {
    contents
        .match_indices(['"', '\''])
        .filter_map(move |(at, quote)| {
            let before = contents[..at].trim_end();
            let before = before.strip_suffix('(').unwrap_or(before).trim_end();
            if !(before.ends_with("from") || before.ends_with("import")) {
                return None;
            }
            let rest = &contents[at + quote.len()..];
            let specifier = &rest[..rest.find(quote)?];
            (specifier.starts_with("./") || specifier.starts_with("../")).then_some(specifier)
        })
}

fn resolve(path: &str, specifier: &str) -> [String; 5] {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    for part in specifier.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let base = parts.join("/");
    ["", ".ts", ".tsx", "/index.ts", "/index.tsx"].map(|end| format!("{base}{end}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_imports_resolve_to_their_files_and_the_rest_are_left_out() {
        let code = r#"import { a } from "./sdk";
import type { B } from '../shared/b.ts';
import "./side";
const later = await import("./later");
export * from "./again";
const words = "from ./not-an-import";
import { c } from "some-package";"#;
        let imported = imported("extensions/song/page.tsx", code);
        for path in [
            "extensions/song/sdk.ts",
            "extensions/shared/b.ts",
            "extensions/song/side.tsx",
            "extensions/song/later/index.ts",
            "extensions/song/again.ts",
        ] {
            assert!(imported.contains(&path.to_string()), "{path}: {imported:?}");
        }
        assert_eq!(imported.len(), 5 * 5);
    }
}
