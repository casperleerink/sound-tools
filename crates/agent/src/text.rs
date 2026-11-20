//! Two rules of the agent's text a view keeps, whatever draws its markdown.

/// A run of a line the app writes, such as a question of the agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    /// Mono on a fill.
    pub code: bool,
}

/// One line the app writes, such as "Run `cargo build`": only the text between two backticks
/// is styled, as code, and nothing else is markdown. A `*` in a command or a pattern stays a
/// star, and a backtick with no partner shows as it is.
pub fn code_spans(line: &str) -> Vec<Span> {
    let parts: Vec<&str> = line.split('`').collect();
    let last = parts.len().saturating_sub(1);
    let mut spans: Vec<Span> = Vec::new();
    for (index, part) in parts.into_iter().enumerate() {
        let (text, code) = match (index % 2 == 1, index < last) {
            (true, true) => (part.to_string(), true),
            (true, false) => (format!("`{part}"), false),
            (false, _) => (part.to_string(), false),
        };
        match spans.last_mut() {
            _ if text.is_empty() => {}
            Some(previous) if previous.code == code => previous.text.push_str(&text),
            _ => spans.push(Span { text, code }),
        }
    }
    spans
}

/// Only web and mail links open. A link in an answer is written by the agent, and a `file:`
/// or app URL could start something on the machine.
pub fn link_opens(url: &str) -> bool {
    ["https://", "http://", "mailto:"]
        .iter()
        .any(|scheme| url.starts_with(scheme))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(line: &str) -> Vec<(String, bool)> {
        (code_spans(line).into_iter())
            .map(|span| (span.text, span.code))
            .collect()
    }

    fn expected(spans: &[(&str, bool)]) -> Vec<(String, bool)> {
        (spans.iter())
            .map(|(text, code)| (text.to_string(), *code))
            .collect()
    }

    #[test]
    fn a_line_of_the_app_styles_only_its_code() {
        assert_eq!(
            spans("Run `cargo build` on *.rs"),
            expected(&[("Run ", false), ("cargo build", true), (" on *.rs", false)])
        );
        // A backtick with no partner shows as it is.
        assert_eq!(
            spans("Run `ls` and `this"),
            expected(&[("Run ", false), ("ls", true), (" and `this", false)])
        );
        assert_eq!(spans(""), expected(&[]));
    }

    #[test]
    fn only_web_and_mail_links_open() {
        for opens in ["https://example.com", "http://example.com", "mailto:a@b.c"] {
            assert!(link_opens(opens), "{opens}");
        }
        for stays in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "sound-tools://x",
            "/tmp",
        ] {
            assert!(!link_opens(stays), "{stays}");
        }
    }
}
