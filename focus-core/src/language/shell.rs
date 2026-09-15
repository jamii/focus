//! Tokenizer for sh and bash.
//!
//! Shell is mostly words, so this leans the other way from the other
//! tokenizers: anything that is not quoting, punctuation or a bracket is a
//! word, and a word can hold the `.`, `/` and `-` that paths and flags are
//! made of. Heredocs are not handled - `<<EOF` reads as punctuation and a
//! word, and the body reads as ordinary shell.

use super::{Flow, Lexer, TokenKind, bracket};

pub(super) fn next_token(lexer: &mut Lexer) -> TokenKind {
    // `#` only starts a comment at the start of a word. Inside one it is
    // an ordinary character: `${#argv}`, `git log HEAD#`.
    if lexer.peek() == Some(b'#') && starts_a_word(lexer) {
        lexer.eat_line();
        return TokenKind::Comment;
    }
    // `$'...'` takes escapes where a plain `'...'` does not.
    if lexer.eat_str(b"$'") {
        return lexer.eat_quoted(b'\'');
    }

    let start = lexer.pos;
    let byte = lexer.bump().expect("called at the end of the text");
    if let Some(kind) = bracket(byte) {
        return kind;
    }
    match byte {
        // Nothing is an escape inside a single-quoted string, not even a
        // backslash: it runs to the next quote whatever is in the way.
        b'\'' => {
            lexer.eat_while(|byte| byte != b'\'');
            if lexer.bump().is_none() {
                return TokenKind::Error;
            }
            TokenKind::String
        }
        b'"' => lexer.eat_quoted(b'"'),
        b'`' => lexer.eat_quoted(b'`'),
        byte if byte.is_ascii_whitespace() => {
            lexer.eat_while(|byte| byte.is_ascii_whitespace());
            TokenKind::Whitespace
        }
        byte if byte.is_ascii_digit() => {
            lexer.eat_while(is_word);
            TokenKind::Number
        }
        byte if is_word_start(byte) => {
            lexer.eat_while(is_word);
            let word: &[u8] = &lexer.text[start..lexer.pos];
            match word {
                b"return" | b"exit" => TokenKind::Flow(Flow::Return),
                b"break" | b"continue" => TokenKind::Flow(Flow::Jump),
                word if KEYWORDS.contains(&word) => TokenKind::Keyword,
                _ => TokenKind::Identifier,
            }
        }
        // `;;` ends a `case` arm and is one thing, where `;` on its own
        // just separates two commands.
        b';' => {
            lexer.eat(b';');
            TokenKind::Punctuation
        }
        _ => {
            lexer.eat_while(is_punctuation);
            TokenKind::Punctuation
        }
    }
}

/// Whether the lexer is at the start of a word, which is what decides
/// whether a `#` is a comment.
fn starts_a_word(lexer: &Lexer) -> bool {
    match lexer.pos.checked_sub(1) {
        None => true,
        Some(before) => matches!(
            lexer.text[before],
            b' ' | b'\t' | b'\n' | b'\r' | b';' | b'&' | b'|' | b'(' | b')'
        ),
    }
}

/// Words hold what paths and flags are made of, so that `./build.sh` and
/// `--verbose` read as one thing each rather than as punctuation with a
/// name stuck to it. `-` and `.` cannot start one, or every flag would be
/// a word rather than an option.
fn is_word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn is_word(byte: u8) -> bool {
    is_word_start(byte) || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'/')
}

fn is_punctuation(byte: u8) -> bool {
    !byte.is_ascii_whitespace()
        && !is_word(byte)
        && bracket(byte).is_none()
        && !matches!(byte, b'\'' | b'"' | b'`' | b'#')
        // `;` separates statements and `\` continues a line, so both
        // decide where the line below them goes and have to stand alone
        // rather than joining a run like `;;` or `2>&1`.
        && !matches!(byte, b';' | b'\\')
}

const KEYWORDS: [&[u8]; 19] = [
    b"case",
    b"do",
    b"done",
    b"elif",
    b"else",
    b"esac",
    b"export",
    b"fi",
    b"for",
    b"function",
    b"if",
    b"in",
    b"local",
    b"readonly",
    b"return",
    b"select",
    b"then",
    b"until",
    b"while",
];
