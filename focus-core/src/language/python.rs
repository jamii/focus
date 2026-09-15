//! Tokenizer for Python.

use super::{Flow, Lexer, TokenKind, bracket, is_identifier_continue, is_identifier_start};

pub(super) fn next_token(lexer: &mut Lexer) -> TokenKind {
    if lexer.eat_str(b"#") {
        lexer.eat_line();
        return TokenKind::Comment;
    }
    // `r"..."`, `f"""..."""`, `rb'...'`: a string can carry up to two
    // prefix letters, which would otherwise read as a name.
    if let Some(kind) = prefixed(lexer) {
        return kind;
    }

    let start = lexer.pos;
    let byte = lexer.bump().expect("called at the end of the text");
    if let Some(kind) = bracket(byte) {
        return kind;
    }
    match byte {
        b'"' | b'\'' => string(lexer, byte),
        byte if byte.is_ascii_whitespace() => {
            lexer.eat_while(|byte| byte.is_ascii_whitespace());
            TokenKind::Whitespace
        }
        byte if byte.is_ascii_digit() => number(lexer),
        byte if is_identifier_start(byte) => {
            lexer.eat_while(is_identifier_continue);
            let name: &[u8] = &lexer.text[start..lexer.pos];
            match name {
                b"return" | b"yield" => TokenKind::Flow(Flow::Return),
                b"break" | b"continue" => TokenKind::Flow(Flow::Jump),
                b"raise" => TokenKind::Flow(Flow::Error),
                name if KEYWORDS.contains(&name) => TokenKind::Keyword,
                _ => TokenKind::Identifier,
            }
        }
        _ => {
            lexer.eat_while(is_punctuation);
            TokenKind::Punctuation
        }
    }
}

/// A string with a `r`/`b`/`f`/`u` prefix, or a combination of two of
/// them. None - consuming nothing - if there is no quote after the
/// letters, in which case they were the start of a name.
fn prefixed(lexer: &mut Lexer) -> Option<TokenKind> {
    let is_prefix = |byte: u8| matches!(byte.to_ascii_lowercase(), b'r' | b'b' | b'f' | b'u');
    let mut len = 0;
    while len < 2 && lexer.peek_at(len).is_some_and(is_prefix) {
        len += 1;
    }
    let quote = lexer.peek_at(len)?;
    if len == 0 || !matches!(quote, b'"' | b'\'') {
        return None;
    }
    lexer.pos += len + 1;
    Some(string(lexer, quote))
}

/// A string whose opening quote has been consumed. Three quotes in a row
/// open a triple-quoted string, which runs over as many lines as it likes;
/// a single one ends at the matching quote or at the end of the line.
fn string(lexer: &mut Lexer, quote: u8) -> TokenKind {
    let triple = lexer.peek() == Some(quote) && lexer.peek_at(1) == Some(quote);
    if triple {
        lexer.pos += 2;
        while let Some(byte) = lexer.bump() {
            if byte == b'\\' {
                lexer.bump();
            } else if byte == quote
                && lexer.peek() == Some(quote)
                && lexer.peek_at(1) == Some(quote)
            {
                lexer.pos += 2;
                return TokenKind::String;
            }
        }
        return TokenKind::Error;
    }
    while let Some(byte) = lexer.bump() {
        match byte {
            b'\\' => {
                lexer.bump();
            }
            b'\n' => break,
            byte if byte == quote => return TokenKind::String,
            _ => {}
        }
    }
    TokenKind::Error
}

/// A number whose first digit has been consumed. Bases and suffixes are
/// more alphanumerics: `0x1f`, `1_000`, `1e9`, `3j`.
fn number(lexer: &mut Lexer) -> TokenKind {
    eat_digits(lexer);
    if lexer.peek() == Some(b'.') && lexer.peek_at(1).is_some_and(|byte| byte.is_ascii_digit()) {
        lexer.bump();
        eat_digits(lexer);
    }
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
        && byte != b'#'
        // A statement's `:` decides where the line below it goes, so it
        // has to be a token of its own rather than part of a run like
        // `):` or `]:`.
        && byte != b':'
}

const KEYWORDS: [&[u8]; 35] = [
    b"False",
    b"None",
    b"True",
    b"and",
    b"as",
    b"assert",
    b"async",
    b"await",
    b"break",
    b"class",
    b"continue",
    b"def",
    b"del",
    b"elif",
    b"else",
    b"except",
    b"finally",
    b"for",
    b"from",
    b"global",
    b"if",
    b"import",
    b"in",
    b"is",
    b"lambda",
    b"nonlocal",
    b"not",
    b"or",
    b"pass",
    b"raise",
    b"return",
    b"try",
    b"while",
    b"with",
    b"yield",
];
