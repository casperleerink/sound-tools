//! Which files of a project's code a file imports by a relative path, such as `./song-page`.

/// The paths from the project that the code at `path` may import: for each relative import in
/// `contents`, every file Bun could resolve it to.
pub(super) fn imported(path: &str, contents: &str) -> Vec<String> {
    specifiers(contents)
        .flat_map(|specifier| resolve(path, specifier))
        .collect()
}

/// The endings of code, which an import may leave out.
const CODE: [&str; 6] = [".ts", ".tsx", ".js", ".jsx", ".mts", ".mjs"];

/// Whether a file at `path` is code that can import another.
pub(super) fn is_code(path: &str) -> bool {
    CODE.iter().any(|end| path.ends_with(end))
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

fn resolve(path: &str, specifier: &str) -> Vec<String> {
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
    // Bun takes `./x.js` for `x.ts` too, as TypeScript does.
    let stem = [".js", ".jsx", ".mjs"]
        .iter()
        .find_map(|end| base.strip_suffix(end))
        .unwrap_or(&base);
    let ends = std::iter::once(String::new())
        .chain(CODE.iter().map(|end| end.to_string()))
        .chain(CODE.iter().map(|end| format!("/index{end}")));
    let mut paths: Vec<String> = ends.map(|end| format!("{base}{end}")).collect();
    paths.extend(CODE.iter().map(|end| format!("{stem}{end}")));
    paths
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
import { c } from "some-package";
import { d } from "./d.js";
import { e } from "./e.jsx";
import { f } from "./f";"#;
        let imported = imported("extensions/song/page.tsx", code);
        for path in [
            "extensions/song/sdk.ts",
            "extensions/shared/b.ts",
            "extensions/song/side.tsx",
            "extensions/song/later/index.ts",
            "extensions/song/again.ts",
            "extensions/song/d.ts",
            "extensions/song/e.tsx",
            "extensions/song/f.js",
            "extensions/song/f/index.js",
        ] {
            assert!(imported.contains(&path.to_string()), "{path}: {imported:?}");
        }
        assert!(
            !imported
                .iter()
                .any(|path| path.contains("some-package") || path.contains("not-an-import"))
        );
    }
}
