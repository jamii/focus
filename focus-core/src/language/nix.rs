//! Tokenizer for Nix.

use super::{
    Bracket, Flow, Lexer, TokenKind, bracket, is_identifier_continue, is_identifier_start,
};

pub(super) fn next_token(lexer: &mut Lexer) -> TokenKind {
    if lexer.eat_str(b"#") {
        lexer.eat_line();
        return TokenKind::Comment;
    }
    if lexer.eat_str(b"/*") {
        return block_comment(lexer);
    }
    // `''...''` runs over lines and keeps its own indentation; `\` is not
    // an escape inside one, `''` is.
    if lexer.eat_str(b"''") {
        return indented_string(lexer);
    }
    // A path is bare in Nix: `./foo`, `../foo`, `/nix/store`, `<nixpkgs>`.
    if let Some(kind) = path(lexer) {
        return kind;
    }

    let start = lexer.pos;
    let byte = lexer.bump().expect("called at the end of the text");
    if let Some(kind) = bracket(byte) {
        return kind;
    }
    match byte {
        b'"' => lexer.eat_quoted(b'"', 1),
        byte if byte.is_ascii_whitespace() => {
            lexer.eat_while(|byte| byte.is_ascii_whitespace());
            TokenKind::Whitespace
        }
        byte if byte.is_ascii_digit() => {
            lexer.eat_while(|byte| byte.is_ascii_digit() || byte == b'.');
            TokenKind::Number
        }
        byte if is_identifier_start(byte) => {
            // `-` and `'` are name bytes in Nix: `pkgs.hello-world`, `x'`.
            lexer.eat_while(|byte| is_identifier_continue(byte) || matches!(byte, b'-' | b'\''));
            let name: &[u8] = &lexer.text[start..lexer.pos];
            // `let ... in` is Nix's one block that is not held together
            // by brackets, so it is lexed as though it were: everything
            // that works out of bracket structure then works for it.
            match name {
                b"let" => TokenKind::Open(Bracket::Word),
                b"in" => TokenKind::Close(Bracket::Word),
                // Nix leaves with an error by calling one of these.
                b"throw" | b"abort" => TokenKind::Flow(Flow::Error),
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

/// A block comment whose `/*` has been consumed. Nix's do not nest.
fn block_comment(lexer: &mut Lexer) -> TokenKind {
    while lexer.bump().is_some() {
        if lexer.eat_str(b"*/") {
            return TokenKind::Comment;
        }
    }
    TokenKind::Error
}

/// An indented string whose opening `''` has been consumed. `'''` and
/// `''$` are the escapes, so a `''` followed by either is not the end.
fn indented_string(lexer: &mut Lexer) -> TokenKind {
    while lexer.bump().is_some() {
        if lexer.starts_with(b"''") {
            lexer.pos += 2;
            if matches!(lexer.peek(), Some(b'\'' | b'$')) {
                lexer.bump();
                continue;
            }
            return TokenKind::String {
                open: 2,
                close: Some(2),
            };
        }
    }
    TokenKind::String {
        open: 2,
        close: None,
    }
}

/// A bare path - `./foo`, `/nix/store/x`, `~/.config`, `<nixpkgs>` - which
/// Nix writes without quotes. None, consuming nothing, if what follows is
/// not one: a bare `/` is division and `<` is a comparison.
fn path(lexer: &mut Lexer) -> Option<TokenKind> {
    let is_path_byte =
        |byte: u8| is_identifier_continue(byte) || matches!(byte, b'.' | b'/' | b'-' | b'+' | b'~');
    if lexer.peek() == Some(b'<') {
        let mut ahead = 1;
        while lexer.peek_at(ahead).is_some_and(is_path_byte) {
            ahead += 1;
        }
        if ahead > 1 && lexer.peek_at(ahead) == Some(b'>') {
            lexer.pos += ahead + 1;
            return Some(TokenKind::String {
                open: 1,
                close: Some(1),
            });
        }
        return None;
    }
    // `./`, `../`, `~/` or `/x`: a slash with a path byte after it.
    let leads = match (lexer.peek()?, lexer.peek_at(1)) {
        (b'.', Some(b'/')) | (b'~', Some(b'/')) => 2,
        (b'.', Some(b'.')) if lexer.peek_at(2) == Some(b'/') => 3,
        (b'/', Some(byte)) if is_path_byte(byte) => 2,
        _ => return None,
    };
    lexer.pos += leads;
    lexer.eat_while(is_path_byte);
    // A bare path is a string with nothing around it, so there is no pair
    // of quotes to find the cursor inside.
    Some(TokenKind::String {
        open: 0,
        close: Some(0),
    })
}

fn is_punctuation(byte: u8) -> bool {
    !byte.is_ascii_whitespace()
        && !is_identifier_continue(byte)
        && bracket(byte).is_none()
        && byte != b'"'
        && byte != b'\''
        && byte != b'#'
        // `;` ends an attribute, so it decides where the line below it
        // goes and cannot be swallowed into a run like `);`.
        && byte != b';'
}

const KEYWORDS: [&[u8]; 12] = [
    b"assert",
    b"else",
    b"if",
    b"in",
    b"inherit",
    b"let",
    b"or",
    b"rec",
    b"then",
    b"with",
    b"builtins",
    b"import",
];
