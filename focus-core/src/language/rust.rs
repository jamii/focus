//! Tokenizer for Rust.

use super::{Flow, Lexer, TokenKind, bracket, is_identifier_continue, is_identifier_start};

pub(super) fn next_token(lexer: &mut Lexer) -> TokenKind {
    if lexer.eat_str(b"//") {
        lexer.eat_line();
        return TokenKind::Comment;
    }
    if lexer.eat_str(b"/*") {
        return block_comment(lexer);
    }
    // `b"..."`, `c"..."`, `r"..."`, `br#"..."#`, `b'x'`, `r#ident`: all of
    // them start with a byte that would otherwise read as a name, so they
    // have to be settled before the identifier branch below.
    if let Some(kind) = prefixed(lexer) {
        return kind;
    }

    let start = lexer.pos;
    let byte = lexer.bump().expect("called at the end of the text");
    if let Some(kind) = bracket(byte) {
        return kind;
    }
    match byte {
        b'"' => lexer.eat_quoted(b'"'),
        b'\'' => quote(lexer),
        byte if byte.is_ascii_whitespace() => {
            lexer.eat_while(|byte| byte.is_ascii_whitespace());
            TokenKind::Whitespace
        }
        byte if byte.is_ascii_digit() => number(lexer),
        byte if is_identifier_start(byte) => {
            lexer.eat_while(is_identifier_continue);
            let name: &[u8] = &lexer.text[start..lexer.pos];
            match name {
                b"return" => TokenKind::Flow(Flow::Return),
                b"break" | b"continue" => TokenKind::Flow(Flow::Jump),
                name if KEYWORDS.contains(&name) => TokenKind::Keyword,
                _ => TokenKind::Identifier,
            }
        }
        // `?` leaves the function with an error, which is the whole
        // reason it is worth seeing. `?Sized` is a bound rather than a
        // question, and is the only `?` with a name straight after it.
        b'?' if !lexer.peek().is_some_and(is_identifier_start) => TokenKind::Flow(Flow::Error),
        _ => {
            // Run punctuation together, so that `::`, `->` and `=>` read
            // as one thing - but stop before a comment, which starts with
            // punctuation too.
            while let Some(byte) = lexer.peek() {
                if !is_punctuation(byte) || lexer.starts_with(b"//") || lexer.starts_with(b"/*") {
                    break;
                }
                lexer.bump();
            }
            TokenKind::Punctuation
        }
    }
}

/// A block comment whose `/*` has been consumed. They nest, so this counts
/// rather than looking for the first `*/`.
fn block_comment(lexer: &mut Lexer) -> TokenKind {
    let mut depth = 1;
    while depth > 0 {
        if lexer.eat_str(b"/*") {
            depth += 1;
        } else if lexer.eat_str(b"*/") {
            depth -= 1;
        } else if lexer.bump().is_none() {
            return TokenKind::Error;
        }
    }
    TokenKind::Comment
}

/// The literals that start with a name-shaped prefix. None - consuming
/// nothing - if what follows the prefix is not a literal after all, in
/// which case it was just a name starting with `b`, `c` or `r`.
fn prefixed(lexer: &mut Lexer) -> Option<TokenKind> {
    let prefix = match (lexer.peek()?, lexer.peek_at(1)) {
        (b'b' | b'c', Some(b'r')) => 2,
        (b'b' | b'c' | b'r', _) => 1,
        _ => return None,
    };
    let raw = lexer.peek_at(prefix - 1) == Some(b'r');

    if raw {
        let mut hashes = 0;
        while lexer.peek_at(prefix + hashes) == Some(b'#') {
            hashes += 1;
        }
        match lexer.peek_at(prefix + hashes) {
            Some(b'"') => {
                lexer.pos += prefix + hashes + 1;
                return Some(raw_string(lexer, hashes));
            }
            // `r#match` is a name that happens to spell a keyword.
            Some(byte) if prefix == 1 && hashes == 1 && is_identifier_start(byte) => {
                lexer.pos += 2;
                lexer.eat_while(is_identifier_continue);
                return Some(TokenKind::Identifier);
            }
            _ => return None,
        }
    }

    match lexer.peek_at(prefix) {
        Some(b'"') => {
            lexer.pos += prefix + 1;
            Some(lexer.eat_quoted(b'"'))
        }
        // `b'x'`, but never `c'x'`: there is no such literal.
        Some(b'\'') if lexer.peek() == Some(b'b') => {
            lexer.pos += prefix + 1;
            Some(char_literal(lexer))
        }
        _ => None,
    }
}

/// A raw string whose opening `r##"` has been consumed: it ends at the
/// first `"` followed by the same number of `#`.
fn raw_string(lexer: &mut Lexer, hashes: usize) -> TokenKind {
    while let Some(byte) = lexer.bump() {
        if byte != b'"' {
            continue;
        }
        let mut found = 0;
        while found < hashes && lexer.peek_at(found) == Some(b'#') {
            found += 1;
        }
        if found == hashes {
            lexer.pos += hashes;
            return TokenKind::String;
        }
    }
    TokenKind::Error
}

/// Whatever follows a `'` that has already been consumed. `'a'` is a
/// character and `'a` is a lifetime, and which one this is depends on the
/// byte after the name.
fn quote(lexer: &mut Lexer) -> TokenKind {
    if lexer.peek().is_some_and(is_identifier_start) {
        let mut ahead = 0;
        while lexer.peek_at(ahead).is_some_and(is_identifier_continue) {
            ahead += 1;
        }
        if lexer.peek_at(ahead) == Some(b'\'') {
            lexer.pos += ahead + 1;
            return TokenKind::String;
        }
        lexer.pos += ahead;
        // A lifetime or a loop label, lexed as one identifier with the
        // quote on the front so that it colours as one word. The quote is
        // also what keeps it out of `identifier_at`, which looks for names
        // to search the repo for.
        return TokenKind::Identifier;
    }
    char_literal(lexer)
}

/// A character literal whose opening quote has been consumed.
fn char_literal(lexer: &mut Lexer) -> TokenKind {
    while let Some(byte) = lexer.bump() {
        match byte {
            b'\\' => {
                lexer.bump();
            }
            b'\'' => return TokenKind::String,
            // An unclosed `'` is a typo, not the rest of the file.
            b'\n' => break,
            _ => {}
        }
    }
    TokenKind::Error
}

/// A number whose first digit has been consumed. Suffixes and bases are
/// just more alphanumerics - `0xff`, `1_000u64`, `1e9` all fall out.
fn number(lexer: &mut Lexer) -> TokenKind {
    eat_digits(lexer);
    // A decimal point, but not the `.` of `1..n` or of `1.max(2)`.
    if lexer.peek() == Some(b'.') && lexer.peek_at(1).is_some_and(|byte| byte.is_ascii_digit()) {
        lexer.bump();
        eat_digits(lexer);
    }
    // The sign of an exponent, which is the one place a number contains
    // punctuation: `1e-9`.
    if matches!(lexer.peek(), Some(b'+' | b'-')) && matches!(lexer.text[lexer.pos - 1], b'e' | b'E')
    {
        lexer.bump();
        eat_digits(lexer);
    }
    TokenKind::Number
}

fn eat_digits(lexer: &mut Lexer) {
    lexer.eat_while(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
}

fn is_punctuation(byte: u8) -> bool {
    !byte.is_ascii_whitespace()
        && !is_identifier_continue(byte)
        && bracket(byte).is_none()
        && byte != b'"'
        && byte != b'\''
}

const KEYWORDS: [&[u8]; 39] = [
    b"as",
    b"async",
    b"await",
    b"break",
    b"const",
    b"continue",
    b"crate",
    b"dyn",
    b"else",
    b"enum",
    b"extern",
    b"false",
    b"fn",
    b"for",
    b"if",
    b"impl",
    b"in",
    b"let",
    b"loop",
    b"match",
    b"mod",
    b"move",
    b"mut",
    b"pub",
    b"ref",
    b"return",
    b"self",
    b"Self",
    b"static",
    b"struct",
    b"super",
    b"trait",
    b"true",
    b"type",
    b"union",
    b"unsafe",
    b"use",
    b"where",
    b"while",
];
