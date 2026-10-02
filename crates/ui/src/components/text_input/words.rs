//! Word boundaries for option-arrow, option-backspace and double-click, as macOS text fields
//! find them: a word is a run of letters, digits and underscores, and a jump skips whatever
//! is between words.

use std::ops::Range;

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The start of the word before `offset`, past any spaces and punctuation in between.
pub(super) fn previous_word_start(text: &str, offset: usize) -> usize {
    let before = text.get(..offset).unwrap_or(text);
    let mut start = before.len();
    let mut in_word = false;
    for (index, c) in before.char_indices().rev() {
        if is_word(c) {
            in_word = true;
        } else if in_word {
            break;
        }
        start = index;
    }
    start
}

/// The end of the word after `offset`, past any spaces and punctuation in between.
pub(super) fn next_word_end(text: &str, offset: usize) -> usize {
    let Some(after) = text.get(offset..) else {
        return text.len();
    };
    let mut end = offset;
    let mut in_word = false;
    for (index, c) in after.char_indices() {
        if is_word(c) {
            in_word = true;
        } else if in_word {
            break;
        }
        end = offset + index + c.len_utf8();
    }
    end
}

/// What a double-click at `offset` selects: the word, the run of spaces or the punctuation
/// mark there. Past the end of a line it takes what is before the caret.
pub(super) fn word_at(text: &str, offset: usize) -> Range<usize> {
    #[derive(PartialEq)]
    enum Class {
        Word,
        Space,
        Other,
    }
    let class = |c: char| match c {
        c if is_word(c) => Class::Word,
        c if c.is_whitespace() => Class::Space,
        _ => Class::Other,
    };
    let after = text.get(offset..).and_then(|after| after.chars().next());
    let before = || text.get(..offset)?.char_indices().next_back();
    let Some((at, c)) = after
        .map(|c| (offset, c))
        .filter(|&(_, c)| c != '\n')
        .or_else(before)
        .filter(|&(_, c)| c != '\n')
    else {
        return offset..offset;
    };
    let target = class(c);
    // Punctuation is one mark at a time; words and spaces extend.
    if target == Class::Other {
        return at..at + c.len_utf8();
    }
    let same = |c: char| c != '\n' && class(c) == target;
    let start = text[..at]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| same(c))
        .last()
        .map_or(at, |(index, _)| index);
    let end = text[at..]
        .char_indices()
        .take_while(|&(_, c)| same(c))
        .last()
        .map_or(at, |(index, c)| at + index + c.len_utf8());
    start..end
}

/// The hard line around `offset`, without its newline. A triple-click selects it.
pub(super) fn line_at(text: &str, offset: usize) -> Range<usize> {
    let (Some(before), Some(after)) = (text.get(..offset), text.get(offset..)) else {
        return offset..offset;
    };
    let start = before.rfind('\n').map_or(0, |index| index + 1);
    let end = after.find('\n').map_or(text.len(), |index| offset + index);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_arrows_jump_to_word_edges_past_spaces_and_punctuation() {
        let text = "foo  bar.baz\nqux";
        assert_eq!(previous_word_start(text, text.len()), 13);
        assert_eq!(previous_word_start(text, 13), 9);
        assert_eq!(previous_word_start(text, 9), 5);
        assert_eq!(previous_word_start(text, 5), 0);
        assert_eq!(previous_word_start(text, 0), 0);
        assert_eq!(next_word_end(text, 0), 3);
        assert_eq!(next_word_end(text, 3), 8);
        assert_eq!(next_word_end(text, 8), 12);
        assert_eq!(next_word_end(text, 12), 16);
        assert_eq!(next_word_end(text, 16), 16);
    }

    #[test]
    fn words_of_text_with_more_bytes_to_a_character() {
        let text = "héllo wörld";
        assert_eq!(previous_word_start(text, text.len()), 7);
        assert_eq!(next_word_end(text, 0), 6);
    }

    #[test]
    fn a_double_click_takes_the_word_the_spaces_or_one_mark() {
        let text = "foo  bar..\nbaz";
        assert_eq!(word_at(text, 1), 0..3);
        // Between "foo" and the spaces the character after wins.
        assert_eq!(word_at(text, 3), 3..5);
        assert_eq!(word_at(text, 5), 5..8);
        assert_eq!(word_at(text, 8), 8..9);
        // Past the end of the line: the mark before it.
        assert_eq!(word_at(text, 10), 9..10);
        assert_eq!(word_at(text, 14), 11..14);
        assert_eq!(word_at("", 0), 0..0);
        assert_eq!(word_at("a\n\nb", 2), 2..2);
    }

    #[test]
    fn a_triple_click_takes_the_line() {
        let text = "one\ntwo two\nthree";
        assert_eq!(line_at(text, 6), 4..11);
        assert_eq!(line_at(text, 0), 0..3);
        assert_eq!(line_at(text, text.len()), 12..17);
    }
}
