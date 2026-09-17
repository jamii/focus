//! Syntax highlighting and paren structure.
//!
//! A buffer's colours come from one of three places, which is what
//! `Highlight` says: nothing at all, a tokenizer for the language the
//! buffer's file is in, or spans written directly by the page that
//! generates the buffer - the diff page colours its own +/- lines rather
//! than pretending they are source code.
//!
//! The tokenizers are tokenizers, not parsers: a flat run of tokens plus
//! which brackets pair with which. That is enough for colours, for
//! indentation, for jumping between brackets, and for finding the
//! identifier under the cursor to search the repo for - and it costs a few
//! hundred lines per language with nothing to keep in sync with a compiler.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::path::Path;

use bstr::{BStr, ByteSlice};

use crate::style::{
    COMMENT_COLOR, EMPHASIS_GREEN, EMPHASIS_RED, EMPHASIS_YELLOW, TEXT_COLOR, UNMATCHED_COLOR, hsla,
};

mod markdown;
mod nix;
mod python;
mod rust;
mod shell;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Language {
    Rust,
    Python,
    Shell,
    Nix,
    /// A document rather than code: parsed into coloured ranges, with no
    /// tokens, no brackets to match and no statements. The methods below
    /// that ask about those have nothing to say about it.
    Markdown,
}

/// Where a buffer's colours come from.
#[derive(Clone)]
pub(crate) enum Highlight {
    /// No colours. Scratch buffers, input fields, and files in a language
    /// we have no tokenizer for.
    Plain,
    /// Tokenized from the buffer's text, rebuilt whenever the text
    /// changes.
    Language { language: Language, tokens: Tokens },
    /// Parsed from the buffer's text into the colours it asks for, and
    /// rebuilt the same way - which is what makes this a different thing
    /// from the spans a page writes below, even though they hold the
    /// same. Markdown, whose parts nest rather than following one
    /// another.
    Document {
        language: Language,
        spans: Vec<Span>,
    },
    /// Colours written by the page that generates the buffer, at the same
    /// time as it writes the text. Sorted by offset and non-overlapping.
    /// Offsets move with the text through `OffsetDiff`, like cursors do,
    /// so streaming output keeps the colours it already has.
    Spans(Vec<Span>),
}

/// The two ends of whatever the cursor is inside: a pair of brackets, or
/// the quotes around a string. Always two ends that belong together - a
/// token with nothing to pair with is not one end of anything, and says
/// so in its own colour wherever the cursor happens to be.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Pair {
    pub(crate) open: Range<usize>,
    pub(crate) close: Range<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct Span {
    pub(crate) range: Range<usize>,
    pub(crate) color: [u8; 4],
}

/// Every token of a buffer, in order, and how its brackets pair up.
#[derive(Clone)]
pub(crate) struct Tokens {
    kind: Vec<TokenKind>,
    /// Byte offset of the start of each token. Tokens are contiguous and
    /// cover the whole text - whitespace and comments are tokens - so
    /// token `ix` spans `start[ix]..start[ix + 1]`, and `start` carries one
    /// extra entry holding the length of the text.
    start: Vec<usize>,
    /// For a bracket, the index of the bracket it pairs with. None for
    /// every other token, and for a bracket with nothing to pair with.
    paren_match: Vec<Option<usize>>,
    /// The innermost bracket pair this token is inside, named by its
    /// opening bracket. A bracket is not inside itself.
    paren_parent: Vec<Option<usize>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TokenKind {
    Whitespace,
    Comment,
    /// A string, and how long the quotes around it are: `open` bytes at
    /// the front and `close` at the back, which is what the pair around
    /// the cursor highlights. They are not the same length - Rust's
    /// `r#"..."#` opens with three bytes and closes with two - and they
    /// are not always there: a Nix path is a string with no quotes at
    /// all, and a string still being typed has no closing quote yet,
    /// which is what `None` says.
    String {
        open: u8,
        close: Option<u8>,
    },
    Number,
    Keyword,
    /// A keyword that moves control somewhere else. Worth telling apart
    /// from the rest of the keywords, because where a function leaves is
    /// the first thing anyone looks for when reading one.
    Flow(Flow),
    Identifier,
    Open(Bracket),
    Close(Bracket),
    Punctuation,
    /// Text the tokenizer could not make sense of: an unterminated string
    /// or block comment, a stray byte.
    Error,
}

/// How a language says a statement has ended. The two are not the same
/// question asked twice: in Rust a line break means nothing and a
/// statement runs on until something stops it, while in Nix a line break
/// finishes what was being said unless an operator is left hanging.
enum StatementEnd {
    /// A statement runs on until a line ends with one of these bytes.
    Terminated(&'static [u8]),
    /// A line has finished a statement unless it ends with one of these
    /// tokens.
    Continued(&'static [&'static str]),
    /// A line break ends a statement by itself, so nothing carries on.
    Never,
}

/// How a language says where a line belongs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IndentStyle {
    /// Brackets and statements decide. A line sits inside the bracket
    /// pair around it, or carries on the statement the line above left
    /// unfinished. Rust and Nix, where a line break means nothing.
    Structure,
    /// The line above decides: its own indent, one step in if it opened a
    /// block, one step out if this line closes one. Python and shell,
    /// where a line break ends a statement and the indent of a line is
    /// the only thing saying which block it is in - so it is the user's
    /// to choose, and this is a guess at what they meant, not a rule.
    Lines,
    /// The line above, read as text: its indent, plus any list or quote
    /// marker on it, so that what is typed next lines up with what it is
    /// carrying on. Markdown, which has no tokens to go on.
    Document,
}

/// Which way control goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Flow {
    /// Leaves with an answer: `return`, Python's `yield`.
    Return,
    /// Goes somewhere else without leaving: `break`, `continue`.
    Jump,
    /// Leaves with an error: `raise`, `throw`, Rust's `?`.
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Bracket {
    Round,
    Square,
    Curly,
    /// A block held together by words rather than brackets: Nix's `let
    /// ... in`. Paired and nested the same way, so everything built on
    /// bracket structure - matching, indenting, what a line sits inside -
    /// works for these without knowing they are words.
    Word,
}

impl Language {
    pub(crate) fn from_path(path: &Path) -> Option<Language> {
        match path.extension()?.as_encoded_bytes() {
            b"rs" => Some(Language::Rust),
            b"py" | b"pyi" => Some(Language::Python),
            b"sh" | b"bash" => Some(Language::Shell),
            b"nix" => Some(Language::Nix),
            b"md" | b"markdown" => Some(Language::Markdown),
            _ => None,
        }
    }

    /// Whether a bracket with nothing to pair with is worth drawing as
    /// an error. Shell writes `)` without a `(` - every `case` pattern
    /// ends with one - so a red paren on every arm would be noise rather
    /// than news.
    fn brackets_must_pair(self) -> bool {
        !matches!(self, Language::Shell)
    }

    /// Whether `<` and `>` are sometimes brackets. Rust writes generic
    /// arguments in them; every other language here writes only
    /// comparisons with them.
    fn has_angle_brackets(self) -> bool {
        matches!(self, Language::Rust)
    }

    /// Whether reading the text gives tokens. Markdown gives coloured
    /// ranges instead, which is what `Highlight` chooses between.
    fn is_tokenized(self) -> bool {
        !matches!(self, Language::Markdown)
    }

    /// How the language says where a line belongs.
    pub(crate) fn indent_style(self) -> IndentStyle {
        match self {
            Language::Rust | Language::Nix => IndentStyle::Structure,
            Language::Python | Language::Shell => IndentStyle::Lines,
            Language::Markdown => IndentStyle::Document,
        }
    }

    /// Token texts that open a block when a line ends with one: Python's
    /// `:`, shell's `then` and `do`, and the brackets, whose contents are
    /// a block of their own. `Lines` languages only.
    fn block_open(self) -> &'static [&'static str] {
        match self {
            Language::Python => &[":", "(", "[", "{", "\\"],
            Language::Markdown => &[],
            // `)` ends a `case` pattern, which opens the arm below it.
            Language::Shell => &[
                "then", "do", "else", "in", "{", "(", "[", ")", "&&", "||", "|", "\\",
            ],
            Language::Rust | Language::Nix => &[],
        }
    }

    /// Token texts that end the block the line above was in, so that the
    /// line below steps back out: shell's `;;` ends a `case` arm, and
    /// what follows is the next pattern.
    fn block_dedent_after(self) -> &'static [&'static str] {
        match self {
            Language::Shell => &[";;"],
            Language::Python | Language::Rust | Language::Nix | Language::Markdown => &[],
        }
    }

    /// Words that close a block when a line starts with one, and the
    /// words that could have opened what they close. `Lines` languages
    /// only: with no closing bracket to go on, a word is all there is to
    /// say that a block has ended - and how far back out it goes is
    /// decided by where that block started, not by a fixed step. A one
    /// step guess would put an `elif` under the `for` it is not part of.
    pub(crate) fn block_close(self) -> &'static [(&'static str, &'static [&'static str])] {
        match self {
            Language::Python => &[
                ("elif", &["if", "elif"]),
                ("else", &["if", "elif", "for", "while", "try"]),
                ("except", &["try", "except"]),
                ("finally", &["try", "except", "else"]),
                ("case", &["match", "case"]),
            ],
            Language::Shell => &[
                ("fi", &["if", "elif", "else"]),
                ("elif", &["if", "elif"]),
                ("else", &["if", "elif"]),
                ("done", &["for", "while", "until", "select"]),
                ("esac", &["case"]),
            ],
            Language::Rust | Language::Nix | Language::Markdown => &[],
        }
    }

    /// How the language says a statement has ended, which is what
    /// decides whether the line below carries one on.
    fn statement_end(self) -> StatementEnd {
        match self {
            // Rust runs on until it is stopped: most lines end in the
            // middle of an expression, and the ones that finish something
            // say so.
            Language::Rust => StatementEnd::Terminated(b";,}"),
            // Nix applies a function to an argument by writing them next
            // to each other, so a line ending in a name has finished
            // saying something. Only an operator left dangling carries
            // on. A lambda's `:` is not one of them: `{ pkgs }:` ends a
            // header, and its body starts back at the left.
            Language::Nix => StatementEnd::Continued(&[
                "=", "+", "++", "//", "?", "||", "&&", "->", "==", "!=", "or", "then", "else",
                "with", "assert",
            ]),
            // A line break ends a statement outright.
            Language::Python | Language::Shell | Language::Markdown => StatementEnd::Never,
        }
    }

    /// Words that close a condition that ran over several lines and open
    /// its block, so that a line starting with one sits back where the
    /// statement started rather than one step in: the `else` of a Rust
    /// `let ... else`, the `in` of a Nix `let`.
    pub(crate) fn block_keyword(self) -> &'static [&'static str] {
        match self {
            Language::Rust => &["else"],
            Language::Nix => &["in"],
            Language::Python | Language::Shell | Language::Markdown => &[],
        }
    }

    /// A line whose first code token starts with one of these stands on
    /// its own, however it ends. Rust's `#[attributes]` sit on a line
    /// above the item they are about, and end in `]` rather than in
    /// anything that finishes a statement - without this the item below
    /// would read as carrying the attribute on.
    fn standalone_line_start(self) -> &'static [u8] {
        match self {
            Language::Rust => b"#",
            Language::Python | Language::Shell | Language::Nix | Language::Markdown => b"",
        }
    }

    /// How the language starts a comment that runs to the end of the
    /// line: what ctrl+/ writes and ctrl+shift+/ takes away. Markdown
    /// has none - its comments are HTML, which needs a closer too.
    pub(crate) fn line_comment(self) -> Option<&'static str> {
        match self {
            Language::Rust => Some("//"),
            Language::Python | Language::Shell | Language::Nix => Some("#"),
            Language::Markdown => None,
        }
    }

    pub(crate) fn indent_width(self) -> usize {
        match self {
            Language::Rust | Language::Python => 4,
            Language::Nix | Language::Shell | Language::Markdown => 2,
        }
    }

    /// Read one token, leaving the lexer just past it. Only called with
    /// text left to read, and must consume at least one byte.
    fn next_token(self, lexer: &mut Lexer) -> TokenKind {
        match self {
            Language::Rust => rust::next_token(lexer),
            Language::Python => python::next_token(lexer),
            Language::Shell => shell::next_token(lexer),
            Language::Nix => nix::next_token(lexer),
            Language::Markdown => panic!("markdown is parsed, not tokenized"),
        }
    }
}

impl Highlight {
    pub(crate) fn new(language: Option<Language>, text: &BStr) -> Highlight {
        match language {
            None => Highlight::Plain,
            Some(language) if language.is_tokenized() => Highlight::Language {
                language,
                tokens: Tokens::new(language, text),
            },
            Some(language) => Highlight::Document {
                language,
                spans: markdown::spans(text),
            },
        }
    }

    /// The language the colours come from, if they come from one.
    pub(crate) fn language(&self) -> Option<Language> {
        match self {
            Highlight::Language { language, .. } | Highlight::Document { language, .. } => {
                Some(*language)
            }
            Highlight::Plain | Highlight::Spans(_) => None,
        }
    }

    /// Read the text again, for a buffer whose colours come from it.
    pub(crate) fn refresh(&mut self, text: &BStr) {
        match self {
            Highlight::Language { language, tokens } => *tokens = Tokens::new(*language, text),
            Highlight::Document { spans, .. } => *spans = markdown::spans(text),
            Highlight::Plain | Highlight::Spans(_) => {}
        }
    }

    pub(crate) fn assert_invariants(&self, text: &BStr) {
        match self {
            Highlight::Plain => {}
            Highlight::Language { tokens, .. } => tokens.assert_invariants(text),
            Highlight::Document { spans, .. } | Highlight::Spans(spans) => {
                // Sorted and non-overlapping, so `color_runs` can binary
                // search, and inside the text, so a page can never colour
                // bytes that are no longer there.
                for pair in spans.windows(2) {
                    assert!(pair[0].range.end <= pair[1].range.start);
                }
                for span in spans {
                    assert!(span.range.start < span.range.end);
                    assert!(span.range.end <= text.len());
                }
            }
        }
    }

    /// The colour of every byte in `range`, as the longest runs of one
    /// colour that cover it exactly, in order. Runs are clipped to `range`,
    /// and anything with no colour of its own comes back as `TEXT_COLOR`.
    ///
    /// `out` is cleared first and reused across the lines of a frame, so
    /// drawing a screenful allocates nothing.
    pub(crate) fn color_runs(
        &self,
        text: &BStr,
        range: Range<usize>,
        out: &mut Vec<(Range<usize>, [u8; 4])>,
    ) {
        out.clear();
        match self {
            Highlight::Plain => push_run(out, range, TEXT_COLOR),
            Highlight::Language { language, tokens } => {
                let Some(first) = tokens.token_containing(range.start) else {
                    push_run(out, range, TEXT_COLOR);
                    return;
                };
                for ix in first..tokens.len() {
                    let token = tokens.range(ix);
                    if token.start >= range.end {
                        break;
                    }
                    let clipped = token.start.max(range.start)..token.end.min(range.end);
                    push_run(out, clipped, tokens.color(text, ix, *language));
                }
            }
            Highlight::Document { spans, .. } | Highlight::Spans(spans) => {
                let first = spans.partition_point(|span| span.range.end <= range.start);
                let mut pos = range.start;
                for span in &spans[first..] {
                    if span.range.start >= range.end {
                        break;
                    }
                    push_run(out, pos..span.range.start.min(range.end), TEXT_COLOR);
                    let end = span.range.end.min(range.end);
                    push_run(out, pos.max(span.range.start)..end, span.color);
                    pos = end;
                }
                push_run(out, pos..range.end, TEXT_COLOR);
            }
        }
    }
}

fn push_run(out: &mut Vec<(Range<usize>, [u8; 4])>, range: Range<usize>, color: [u8; 4]) {
    if range.start >= range.end {
        return;
    }
    if let Some((last_range, last_color)) = out.last_mut()
        && *last_color == color
        && last_range.end == range.start
    {
        last_range.end = range.end;
        return;
    }
    out.push((range, color));
}

impl Tokens {
    pub(crate) fn new(language: Language, text: &BStr) -> Tokens {
        let mut kind = Vec::new();
        let mut start = Vec::new();
        let mut lexer = Lexer { text, pos: 0 };
        while !lexer.at_end() {
            let pos = lexer.pos;
            start.push(pos);
            kind.push(language.next_token(&mut lexer));
            // A tokenizer that returns without consuming anything would
            // spin here forever, so say so at the point it happens rather
            // than hanging the editor.
            assert!(
                lexer.pos > pos,
                "{:?} tokenizer made no progress at {}",
                language,
                pos
            );
        }
        start.push(text.len());

        let mut paren_match = vec![None; kind.len()];
        let mut paren_parent = vec![None; kind.len()];
        let mut stack: Vec<usize> = Vec::new();
        for ix in 0..kind.len() {
            // A closing bracket pairs with the nearest opening one of its
            // own kind, which is not always the innermost one still open.
            // In `vec![0u8; (w * [ h) as usize];` the middle `[` is the
            // typo, and the `(` and `)` either side of it are a pair that
            // says so: the brackets left open inside the one that just
            // closed are the ones that went wrong, and they are what ends
            // up drawn red. A closing bracket with no opening one of its
            // kind anywhere still open - a shell `case` pattern, a stray
            // `)` - pairs with nothing and closes nothing, rather than
            // taking the structure of everything below it with it.
            if let TokenKind::Close(closing) = kind[ix]
                && let Some(pos) = stack.iter().rposition(|open_ix| {
                    matches!(kind[*open_ix], TokenKind::Open(opening) if opening == closing)
                })
            {
                let open_ix = stack[pos];
                stack.truncate(pos);
                paren_match[ix] = Some(open_ix);
                paren_match[open_ix] = Some(ix);
            }
            paren_parent[ix] = stack.last().copied();
            if matches!(kind[ix], TokenKind::Open(_)) {
                stack.push(ix);
            }
        }

        if language.has_angle_brackets() {
            pair_angles(&kind, &start, text, &mut paren_match);
        }

        let tokens = Tokens {
            kind,
            start,
            paren_match,
            paren_parent,
        };
        if cfg!(debug_assertions) {
            tokens.assert_invariants(text);
        }
        tokens
    }

    pub(crate) fn assert_invariants(&self, text: &BStr) {
        assert_eq!(self.start.len(), self.kind.len() + 1);
        assert_eq!(self.paren_match.len(), self.kind.len());
        assert_eq!(self.paren_parent.len(), self.kind.len());
        assert_eq!(self.start.first().copied().unwrap_or(0), 0);
        assert_eq!(self.start.last().copied().unwrap_or(0), text.len());
        for ix in 0..self.kind.len() {
            // Tokens are contiguous, non-empty and in order, so that an
            // offset lands in exactly one of them and `token_containing`
            // can binary search.
            assert!(self.start[ix] < self.start[ix + 1]);
            // Pairing is symmetric, points the way it says it does, and
            // only ever joins two brackets of the same kind.
            if let Some(match_ix) = self.paren_match[ix] {
                assert_eq!(self.paren_match[match_ix], Some(ix));
                assert!(match_ix != ix);
                let (open_ix, close_ix) = (ix.min(match_ix), ix.max(match_ix));
                match (self.kind[open_ix], self.kind[close_ix]) {
                    (TokenKind::Open(opening), TokenKind::Close(closing)) => {
                        assert_eq!(opening, closing)
                    }
                    // Angle brackets stay the punctuation they look like,
                    // and pair up anyway.
                    (TokenKind::Punctuation, TokenKind::Punctuation) => {
                        assert_eq!(&text[self.range(open_ix)], "<");
                        assert_eq!(&text[self.range(close_ix)], ">");
                    }
                    kinds => panic!("paired {:?}", kinds),
                }
            }
            // A token is inside its parent, and its parent is an opening
            // bracket that is still open where it sits.
            if let Some(parent_ix) = self.paren_parent[ix] {
                assert!(parent_ix < ix);
                assert!(matches!(self.kind[parent_ix], TokenKind::Open(_)));
                match self.paren_match[parent_ix] {
                    Some(close_ix) => assert!(ix < close_ix),
                    None => {}
                }
            }
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.kind.len()
    }

    pub(crate) fn range(&self, ix: usize) -> Range<usize> {
        self.start[ix]..self.start[ix + 1]
    }

    /// The token `offset` lands in. None past the end of the text.
    pub(crate) fn token_containing(&self, offset: usize) -> Option<usize> {
        let ix = self.start.partition_point(|start| *start <= offset);
        ix.checked_sub(1).filter(|ix| *ix < self.kind.len())
    }

    /// The last token starting strictly before `offset`.
    pub(crate) fn token_before(&self, offset: usize) -> Option<usize> {
        self.start
            .partition_point(|start| *start < offset)
            .checked_sub(1)
            .filter(|ix| *ix < self.kind.len())
    }

    /// The first token starting at or after `offset`.
    pub(crate) fn token_after(&self, offset: usize) -> Option<usize> {
        let ix = self.start.partition_point(|start| *start < offset);
        (ix < self.kind.len()).then_some(ix)
    }

    /// True for a bracket with nothing to pair with, which is drawn as an
    /// error: a bracket is missing somewhere, or one of another kind is
    /// in the way.
    fn is_mismatched(&self, ix: usize) -> bool {
        self.paren_match[ix].is_none()
            && matches!(self.kind[ix], TokenKind::Open(_) | TokenKind::Close(_))
    }

    /// The pair of tokens the cursor at `offset` sits between: the quotes
    /// of the string it is in, or else the nearest bracket around it and
    /// the one it pairs with. The cursor sits between characters, so a
    /// bracket it is merely next to is not around it: `foo(bar)|` finds
    /// whatever encloses all of that, not the pair it has just left.
    ///
    /// Only tokens that found a partner: a bracket with nothing to pair
    /// with is not one end of anything, and is already drawn in the
    /// colour that says so.
    pub(crate) fn enclosing_pair(&self, offset: usize) -> Option<Pair> {
        // Inside a string, the quotes around it are nearer than any
        // bracket, and the brackets written inside one are not tokens for
        // the search below to find.
        if let Some(ix) = self.token_containing(offset)
            && let TokenKind::String {
                open,
                close: Some(close),
            } = self.kind[ix]
            && open > 0
            && close > 0
        {
            let token = self.range(ix);
            if token.start < offset && offset < token.end {
                return Some(Pair {
                    open: token.start..token.start + open as usize,
                    close: token.end - close as usize..token.end,
                });
            }
        }

        // Backwards to the nearest bracket still open where the cursor
        // is, stepping over the pairs that closed on the way in one jump
        // each. Which end of its pair a token is is the side its partner
        // is on.
        let mut ix = self.token_before(offset);
        while let Some(candidate) = ix {
            if let Some(match_ix) = self.paren_match[candidate] {
                if match_ix < candidate {
                    // A pair that has already closed is not around the
                    // cursor, and nor is anything inside it.
                    ix = match_ix.checked_sub(1);
                    continue;
                }
                // An opening bracket the cursor is partway through - the
                // `le` of Nix's `let` - is not open around it yet.
                if self.range(candidate).end <= offset {
                    return Some(Pair {
                        open: self.range(candidate),
                        close: self.range(match_ix),
                    });
                }
            }
            ix = candidate.checked_sub(1);
        }
        None
    }

    fn color(&self, text: &BStr, ix: usize, language: Language) -> [u8; 4] {
        match self.kind[ix] {
            // Brackets are structure, not content: greyed out, unless they
            // are the ones that have gone wrong.
            TokenKind::Open(bracket) | TokenKind::Close(bracket) => {
                if self.is_mismatched(ix) && language.brackets_must_pair() {
                    UNMATCHED_COLOR
                } else if bracket == Bracket::Word {
                    // These are words, and read as the keywords they are.
                    TEXT_COLOR
                } else {
                    COMMENT_COLOR
                }
            }
            TokenKind::Comment | TokenKind::Whitespace => COMMENT_COLOR,
            // A string with no closing quote has run off the end of what
            // was meant to be in it, the same way a stray byte or an
            // unclosed block comment has: nothing to pair with.
            TokenKind::String { close: None, .. } | TokenKind::Error => UNMATCHED_COLOR,
            TokenKind::Flow(Flow::Return) => EMPHASIS_GREEN,
            TokenKind::Flow(Flow::Jump) => EMPHASIS_YELLOW,
            TokenKind::Flow(Flow::Error) => EMPHASIS_RED,
            // Every name gets its own colour, the same colour everywhere
            // it appears. Not semantic - nothing here knows what a name
            // means - but enough to pick one out of a page.
            TokenKind::Identifier => ident_color(&text[self.range(ix)]),
            TokenKind::String { .. }
            | TokenKind::Number
            | TokenKind::Keyword
            | TokenKind::Punctuation => TEXT_COLOR,
        }
    }

    /// How far the line covering `line` should be indented. An empty
    /// range is a line about to be created, as Enter does.
    ///
    /// Three rules, in order. A line that starts with a closing bracket
    /// lines up with the line its opening bracket is on. A line that
    /// carries on a statement the line above left unfinished sits one
    /// step in from wherever that statement began. Anything else sits one
    /// step in from the line that opened the bracket pair it is inside.
    // TODO Python and shell indent by block, not by bracket: `:` and
    // `then`/`do` open a block that no bracket closes. They need a hook
    // here, and `statement_end` of None until they have one.
    pub(crate) fn ideal_indent(
        &self,
        text: &BStr,
        line: Range<usize>,
        language: Language,
    ) -> usize {
        // A string that runs over lines - Nix's `''...''`, a Python
        // docstring - holds text, not code, so its layout is part of it
        // rather than something to work out. The line that opens one
        // steps the text in; every line after that repeats the line
        // above, which is as much as anything can say about words it does
        // not understand. The line that closes the string is not inside
        // it and falls through to the rules below.
        if let Some(above_end) = line.start.checked_sub(1)
            && let Some(ix) = self.token_before(line.start)
            && matches!(self.kind[ix], TokenKind::String { .. } | TokenKind::Error)
        {
            let token = self.range(ix);
            let ends_on_this_line = token.end > line.start && token.end <= line.end;
            let above = line_start(text, above_end);
            let indent = text[above..]
                .iter()
                .take_while(|byte| **byte == b' ')
                .count();
            if !ends_on_this_line && text[token.clone()].contains(&b'\n') {
                return indent;
            }
            // Still being typed, so it has not met its closing quote and
            // everything below is inside it: the line it opened on is
            // what the text hangs off.
            if matches!(
                self.kind[ix],
                TokenKind::Error | TokenKind::String { close: None, .. }
            ) && token.start >= above
            {
                return indent + language.indent_width();
            }
        }

        let indent_width = language.indent_width();
        if language.indent_style() == IndentStyle::Lines {
            return self.line_indent_from_above(text, line, language);
        }

        let anchor = 'anchor: {
            // The line starts with a closing bracket: line up with the
            // line its opening bracket is on, not one step in from it.
            if let Some(after_ix) = self.token_after(line.start)
                && self.range(after_ix).start < line.end
                && let Some(match_ix) = self.paren_match[after_ix]
                && match_ix < after_ix
            {
                break 'anchor Some((match_ix, 0));
            }

            let Some(before_ix) = self.token_before(line.start) else {
                // Nothing above this line at all: the top of the file.
                break 'anchor None;
            };

            // What comes just before is an opening bracket, which only
            // happens for a line about to be created: step in from it.
            if matches!(self.kind[before_ix], TokenKind::Open(_)) {
                break 'anchor Some((before_ix, indent_width));
            }

            // The line above did not finish what it was saying, so this
            // line carries on from it - a method chain, a long expression,
            // a `where` clause. One step in from where the statement
            // began, not from the line above, so that the third line of a
            // chain lines up with the second rather than stepping in
            // again.
            if let Some(code_ix) = self.code_token_before(line.start)
                && !self.line_ends_statement(text, code_ix, language)
            {
                let start_ix = self.statement_start(text, code_ix, language);
                // A line that opens a block - `{`, or a word standing in
                // for one - closes the condition that ran over the lines
                // above it, and sits back where that statement started;
                // its body then steps in from there.
                let opens_block = self.token_after(line.start).is_some_and(|ix| {
                    if self.range(ix).start >= line.end {
                        return false;
                    }
                    if self.kind[ix] == TokenKind::Open(Bracket::Curly) {
                        return true;
                    }
                    let word: &[u8] = &text[self.range(ix)];
                    language
                        .block_keyword()
                        .iter()
                        .any(|keyword| word == keyword.as_bytes())
                });
                let added = if opens_block { 0 } else { indent_width };
                break 'anchor Some((start_ix, added));
            }

            // Otherwise sit with the rest of the bracket pair this line is
            // in. `paren_parent` is always an opening bracket.
            if let Some(parent_ix) = self.paren_parent[before_ix] {
                break 'anchor Some((parent_ix, indent_width));
            }

            // Nothing to line up with: the top level of the file.
            None
        };

        let Some((anchor_ix, added_indent)) = anchor else {
            return 0;
        };
        self.line_indent(text, anchor_ix) + added_indent
    }

    /// Where `line` goes in a language whose blocks are held together by
    /// indentation rather than by brackets: alongside the line above it,
    /// one step in if that line opened a block, one step out if this one
    /// closes the block it is in.
    ///
    /// Everything comes from the line above, which means a line put in
    /// the wrong place carries every line under it with it. That is the
    /// bargain in a language where the indent is the structure: nothing
    /// here can know better than the person typing.
    fn line_indent_from_above(&self, text: &BStr, line: Range<usize>, language: Language) -> usize {
        let indent_width = language.indent_width();
        let first_ix = self
            .token_after(line.start)
            .filter(|ix| self.range(*ix).start < line.end);

        if let Some(first_ix) = first_ix {
            // A closing bracket lines up with the line its opening
            // bracket is on, the same as anywhere else.
            if matches!(self.kind[first_ix], TokenKind::Close(_))
                && let Some(match_ix) = self.paren_match[first_ix]
                && match_ix < first_ix
            {
                return self.line_indent(text, match_ix);
            }
            // A word that closes a block goes back to the line that
            // opened it, however many levels up that is.
            let first: &[u8] = &text[self.range(first_ix)];
            if let Some((_, openers)) = language
                .block_close()
                .iter()
                .find(|(word, _)| first == word.as_bytes())
                && let Some(indent) = self.block_opener_indent(text, line.start, openers)
            {
                return indent;
            }
        }

        let Some(above_ix) = self.code_token_before(line.start) else {
            return 0;
        };
        let mut indent = self.line_indent(text, above_ix);
        let above: &[u8] = &text[self.range(above_ix)];
        if language
            .block_open()
            .iter()
            .any(|word| above == word.as_bytes())
        {
            indent += indent_width;
        }
        if language
            .block_dedent_after()
            .iter()
            .any(|word| above == word.as_bytes())
        {
            indent = indent.saturating_sub(indent_width);
        }
        indent
    }

    /// How far the line that opened the block ending at `offset` is
    /// indented. Walks back a line at a time, looking only at lines that
    /// are further out than every line seen so far, so that a block
    /// nested inside this one is stepped over rather than matched.
    fn block_opener_indent(&self, text: &BStr, offset: usize, openers: &[&str]) -> Option<usize> {
        let mut ix = self.code_token_before(offset)?;
        let mut outermost = usize::MAX;
        loop {
            let first_ix = self.first_code_token_on_line(text, ix)?;
            let indent = self.line_indent(text, first_ix);
            if indent < outermost {
                outermost = indent;
                let word: &[u8] = &text[self.range(first_ix)];
                if openers.iter().any(|opener| word == opener.as_bytes()) {
                    return Some(indent);
                }
                if indent == 0 {
                    return None;
                }
            }
            ix = self.code_token_before(line_start(text, self.range(first_ix).start))?;
        }
    }

    /// How far the line holding token `ix` is indented.
    fn line_indent(&self, text: &BStr, ix: usize) -> usize {
        let token_start = self.range(ix).start;
        let start = line_start(text, token_start);
        let mut text_start = start;
        while text_start < token_start && text[text_start] == b' ' {
            text_start += 1;
        }
        text_start - start
    }

    /// The last token before `offset` that is neither whitespace nor a
    /// comment: what the line above actually says, rather than how it was
    /// laid out or what was said about it.
    fn code_token_before(&self, offset: usize) -> Option<usize> {
        let mut ix = self.token_before(offset)?;
        while matches!(self.kind[ix], TokenKind::Whitespace | TokenKind::Comment) {
            ix = ix.checked_sub(1)?;
        }
        Some(ix)
    }

    /// Whether the line that token `ix` is the last code on finishes a
    /// statement, so that the line below starts a new one rather than
    /// carrying it on. Only punctuation and closing brackets can end one:
    /// a name or a literal at the end of a line always has more coming.
    fn line_ends_statement(&self, text: &BStr, ix: usize, language: Language) -> bool {
        if let Some(first_ix) = self.first_code_token_on_line(text, ix)
            && language
                .standalone_line_start()
                .contains(&text[self.range(first_ix).start])
        {
            return true;
        }
        match language.statement_end() {
            StatementEnd::Never => true,
            StatementEnd::Terminated(bytes) => match self.kind[ix] {
                // Only punctuation and closing brackets can end one: a
                // name or a literal at the end of a line always has more
                // coming after it.
                TokenKind::Punctuation | TokenKind::Close(_) => {
                    bytes.contains(&text[self.range(ix).end - 1])
                }
                _ => false,
            },
            StatementEnd::Continued(tokens) => {
                let last: &[u8] = &text[self.range(ix)];
                !tokens.iter().any(|token| last == token.as_bytes())
            }
        }
    }

    /// The first token on the line holding `ix` that is neither
    /// whitespace nor a comment: what the line leads with.
    fn first_code_token_on_line(&self, text: &BStr, ix: usize) -> Option<usize> {
        let line = line_start(text, self.range(ix).start);
        let mut first = None;
        for ix in (0..=ix).rev() {
            if self.range(ix).start < line {
                break;
            }
            if !matches!(self.kind[ix], TokenKind::Whitespace | TokenKind::Comment) {
                first = Some(ix);
            }
        }
        first
    }

    /// The token the statement holding `ix` starts on, found by walking
    /// back a line at a time for as long as each line is a continuation of
    /// the one above it.
    fn statement_start(&self, text: &BStr, mut ix: usize, language: Language) -> usize {
        while let Some(before_ix) = self.line_above_at_depth(text, ix) {
            if self.line_ends_statement(text, before_ix, language) {
                return ix;
            }
            debug_assert!(before_ix < ix);
            ix = before_ix;
        }
        ix
    }

    /// The token the line above the one holding `ix` ends with, at the
    /// same bracket depth as `ix`: a multi-line argument list or pattern
    /// belongs to the line that opened it, not to itself, so the walk
    /// climbs out of one rather than reading its innards as statements.
    /// None when there is nothing above at that depth - `ix` is on the
    /// first line inside its bracket pair, or the first line of the file.
    fn line_above_at_depth(&self, text: &BStr, ix: usize) -> Option<usize> {
        let target = self.paren_parent[ix];
        let mut before_ix = self.code_token_before(line_start(text, self.range(ix).start))?;
        loop {
            if Some(before_ix) == target {
                // The bracket that opened the group this statement is in.
                return None;
            }
            if self.paren_parent[before_ix] == target {
                return Some(before_ix);
            }
            // Deeper than `ix`: climb to the bracket holding it. Running
            // out of brackets to climb means the line above is outside
            // this statement's group, so there is nothing above to join.
            before_ix = self.paren_parent[before_ix]?;
        }
    }
}

/// Every name gets a colour of its own, from its own bytes, so that the
/// same name is the same colour in every buffer without anything having to
/// agree on a palette.
fn ident_color(name: &[u8]) -> [u8; 4] {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    let hash = hasher.finish().reverse_bits();
    hsla((hash % 360) as f64, 1.0, 0.8, 1.0)
}

/// A byte cursor over the text, shared by every tokenizer so that none of
/// them has to get the bookkeeping right twice.
pub(crate) struct Lexer<'a> {
    text: &'a BStr,
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub(crate) fn at_end(&self) -> bool {
        self.pos >= self.text.len()
    }

    pub(crate) fn peek(&self) -> Option<u8> {
        self.peek_at(0)
    }

    pub(crate) fn peek_at(&self, ahead: usize) -> Option<u8> {
        self.text.get(self.pos + ahead).copied()
    }

    pub(crate) fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.pos += 1;
        Some(byte)
    }

    /// Consume `byte` if it is next, and say whether it was.
    pub(crate) fn eat(&mut self, byte: u8) -> bool {
        let found = self.peek() == Some(byte);
        if found {
            self.pos += 1;
        }
        found
    }

    pub(crate) fn eat_while(&mut self, mut f: impl FnMut(u8) -> bool) {
        while self.peek().is_some_and(&mut f) {
            self.pos += 1;
        }
    }

    pub(crate) fn starts_with(&self, prefix: &[u8]) -> bool {
        self.text[self.pos..].starts_with(prefix)
    }

    /// Consume `prefix` if it is next, and say whether it was.
    pub(crate) fn eat_str(&mut self, prefix: &[u8]) -> bool {
        let found = self.starts_with(prefix);
        if found {
            self.pos += prefix.len();
        }
        found
    }

    /// Read to the end of a quoted string whose opening quote has already
    /// been consumed. `open` is how long that quote was, counting any
    /// prefix in front of it: `"` is one byte, `b"` and shell's `$'` are
    /// two. Unterminated if it runs off the end of the text.
    pub(crate) fn eat_quoted(&mut self, quote: u8, open: u8) -> TokenKind {
        while let Some(byte) = self.bump() {
            if byte == b'\\' {
                self.bump();
            } else if byte == quote {
                return TokenKind::String {
                    open,
                    close: Some(1),
                };
            }
        }
        TokenKind::String { open, close: None }
    }

    /// Read to the end of the line, for a line comment.
    pub(crate) fn eat_line(&mut self) {
        self.eat_while(|byte| byte != b'\n');
    }
}

/// Where a new line at `offset` goes in a document: at the indent of the
/// line above, which is where a list item's own marker sits.
pub(crate) fn document_indent(text: &BStr, offset: usize) -> usize {
    markdown::next_line_indent(text, offset)
}

/// Rust writes a generic argument list in `<` and `>`, and writes
/// comparisons with the same two bytes: `a<b, c>(d)` is a call with type
/// arguments or it is two comparisons, and only something that knows what
/// `a` is can say which. The guess here is the one rustfmt makes safe to
/// take - a comparison is written with spaces around it, a generic
/// argument list is written tight against the name in front of it - plus
/// the things a list cannot do: leave the brackets it started in, cross a
/// `;`, or hold a block.
///
/// Pairing only. These stay punctuation tokens, so a `<` that finds
/// nothing is left alone rather than drawn as an error the way an
/// unpaired bracket is, and none of them are structure to indent by: a
/// generic argument list is not a block.
fn pair_angles(
    kind: &[TokenKind],
    start: &[usize],
    text: &BStr,
    paren_match: &mut [Option<usize>],
) {
    // The `<`s that could still turn out to be brackets, and the bracket
    // depth each was seen at.
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    let mut depth: usize = 0;
    for ix in 0..kind.len() {
        match kind[ix] {
            TokenKind::Open(bracket) => {
                depth += 1;
                // `if a<b {` - a list holds types, never a block.
                if bracket == Bracket::Curly {
                    candidates.clear();
                }
            }
            TokenKind::Close(bracket) => {
                depth = depth.saturating_sub(1);
                if bracket == Bracket::Curly {
                    candidates.clear();
                } else {
                    // A list cannot reach out of the brackets it opened
                    // in: `foo(a<b) > c`.
                    candidates.retain(|(_, candidate_depth)| *candidate_depth <= depth);
                }
            }
            TokenKind::Punctuation => {
                let token = &text[start[ix]..start[ix + 1]];
                if token == "<" {
                    if opens_angle(text, start[ix]) {
                        candidates.push((ix, depth));
                    }
                } else if token == ">" {
                    // The innermost `<` still open at this depth. The
                    // ones after it never found a `>` of their own, and a
                    // list cannot close out of order.
                    if closes_angle(text, start[ix])
                        && let Some(pos) = candidates
                            .iter()
                            .rposition(|(_, candidate_depth)| *candidate_depth == depth)
                    {
                        let (open_ix, _) = candidates[pos];
                        candidates.truncate(pos);
                        paren_match[open_ix] = Some(ix);
                        paren_match[ix] = Some(open_ix);
                    }
                } else if token.contains(&b';') {
                    // The statement the `<` was in has ended without it.
                    // Only at its own depth: the `;` of `Vec<[u8; 4]>` is
                    // inside the brackets of an array type.
                    candidates.retain(|(_, candidate_depth)| *candidate_depth < depth);
                }
            }
            _ => {}
        }
    }
}

/// A `<` that opens a generic argument list rather than comparing: tight
/// against the name or the `::` in front of it, and not the first byte of
/// `<<` or `<=`.
fn opens_angle(text: &BStr, pos: usize) -> bool {
    let before = pos.checked_sub(1).map(|ix| text[ix]);
    before.is_some_and(|byte| is_identifier_continue(byte) || byte == b':')
        && !matches!(
            text.get(pos + 1),
            None | Some(b'<' | b'=' | b' ' | b'\t' | b'\n')
        )
}

/// A `>` that closes one: tight against something a type can end with -
/// a name, `)`, `]`, or the `>` of the list inside this one - and not the
/// first byte of `>=`. The `>` of `->` and of `=>` fails the first test,
/// which is what keeps a return type from closing a list.
fn closes_angle(text: &BStr, pos: usize) -> bool {
    let before = pos.checked_sub(1).map(|ix| text[ix]);
    before.is_some_and(|byte| is_identifier_continue(byte) || matches!(byte, b')' | b']' | b'>'))
        && text.get(pos + 1) != Some(&b'=')
}

/// The offset the line holding `offset` starts at.
fn line_start(text: &BStr, offset: usize) -> usize {
    text[..offset].rfind_byte(b'\n').map_or(0, |ix| ix + 1)
}

pub(crate) fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

pub(crate) fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit()
}

pub(crate) fn bracket(byte: u8) -> Option<TokenKind> {
    match byte {
        b'(' => Some(TokenKind::Open(Bracket::Round)),
        b'[' => Some(TokenKind::Open(Bracket::Square)),
        b'{' => Some(TokenKind::Open(Bracket::Curly)),
        b')' => Some(TokenKind::Close(Bracket::Round)),
        b']' => Some(TokenKind::Close(Bracket::Square)),
        b'}' => Some(TokenKind::Close(Bracket::Curly)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Which `<` and `>` were read as brackets, marked under the line
    /// they are on.
    fn angle_marks(source: &str) -> String {
        let text = BStr::new(source.as_bytes());
        let tokens = Tokens::new(Language::Rust, text);
        let paired: Vec<usize> = (0..tokens.len())
            .filter(|ix| {
                tokens.kind[*ix] == TokenKind::Punctuation && tokens.paren_match[*ix].is_some()
            })
            .map(|ix| tokens.start[ix])
            .collect();
        let mut out = String::new();
        let mut line_start = 0;
        for line in source.lines() {
            let marks: String = (0..line.len())
                .map(|col| {
                    if paired.contains(&(line_start + col)) {
                        line.as_bytes()[col] as char
                    } else {
                        ' '
                    }
                })
                .collect();
            out.push_str(line);
            out.push('\n');
            out.push_str(marks.trim_end());
            out.push('\n');
            line_start += line.len() + 1;
        }
        out
    }

    /// Rust's `<` and `>` are brackets when they hold a generic argument
    /// list and comparisons the rest of the time, told apart by how they
    /// are written rather than by what the names in them mean. Two lines
    /// here are the guess going wrong, and both are lines rustfmt would
    /// not leave as they are: `a<b && c>d` reads as a generic argument
    /// list because a comparison is normally spaced, and `<T as Trait>`
    /// is missed because nothing precedes its `<` to hold it to.
    #[test]
    fn angle_brackets_pair_when_they_are_written_like_brackets() {
        assert_eq!(angle_marks(ANGLE_SAMPLE), ANGLE_MARKS);
    }

    const ANGLE_SAMPLE: &str = "\
let v: Vec<HashMap<K, V>> = Vec::new();
let spaced = a < b && c > d;
let tight = a<b && c>d;
foo::<T>();
if x<y { g(); }
fn f<T: Into<U>>(t: T) -> Result<T, E> { h::<u8>() }
let s = Foo::<'a, T>::new();
let shifted = a << 2 >> 1;
let cmp = a <= b >= c;
let arr: Vec<[u8; 4]> = vec![];
let q = <T as Trait>::f();
let f: Box<dyn Fn(i32) -> i32> = todo!();
";

    const ANGLE_MARKS: &str = "\
let v: Vec<HashMap<K, V>> = Vec::new();
          <       <    >>
let spaced = a < b && c > d;

let tight = a<b && c>d;
             <      >
foo::<T>();
     < >
if x<y { g(); }

fn f<T: Into<U>>(t: T) -> Result<T, E> { h::<u8>() }
    <       < >>                <    >      <  >
let s = Foo::<'a, T>::new();
             <     >
let shifted = a << 2 >> 1;

let cmp = a <= b >= c;

let arr: Vec<[u8; 4]> = vec![];
            <       >
let q = <T as Trait>::f();

let f: Box<dyn Fn(i32) -> i32> = todo!();
          <                  >
";

    /// Whatever the text and wherever the cursor, the pair around it is
    /// either nothing or two ends of the text the cursor is between.
    /// Driven over every offset of each sample, including the offsets
    /// inside a quote and between two bytes of one token.
    #[test]
    fn the_pair_around_the_cursor_is_around_the_cursor() {
        let samples: &[(Language, &str)] = &[
            (Language::Rust, "fn f() { g(x[1], \"s\") }"),
            // Brackets that pair with nothing, in every arrangement.
            (Language::Rust, "((()"),
            (Language::Rust, ")))("),
            (Language::Rust, "(]"),
            (Language::Rust, "([)]"),
            // Quotes of every length, terminated and not.
            (Language::Rust, "let s = r##\"a\"#b\"##;"),
            (Language::Rust, "let s = \"unterminated"),
            (Language::Rust, "let c = b'x'; let l = 'a; let e = '"),
            (Language::Python, "x = f\"\"\"a{b}c\"\"\" + 'y'"),
            (Language::Python, "x = \"\"\"unterminated"),
            // Words as brackets, and a `)` that closes nothing.
            (Language::Nix, "let x = { y = [ 1 ]; }; in x"),
            (Language::Shell, "case $x in\n  a) f;;\n  b) g;;\nesac"),
            // The brackets left open inside one that closed.
            (Language::Rust, "vec![0u8; (w * [ h) as usize];"),
            (Language::Rust, "fn f() { g(x; }"),
            // Angle brackets, which pair up without being brackets.
            (Language::Rust, ANGLE_SAMPLE),
            (Language::Rust, "a<b<c<d"),
            (Language::Rust, "a>b>c>d"),
            (Language::Rust, "Vec<T>"),
            // Nothing to find at all.
            (Language::Rust, ""),
            (Language::Rust, "plain words"),
        ];
        for (language, text) in samples {
            let text = BStr::new(text.as_bytes());
            let tokens = Tokens::new(*language, text);
            for offset in 0..=text.len() {
                let Some(Pair { open, close }) = tokens.enclosing_pair(offset) else {
                    continue;
                };
                // Two ends, in order, with the cursor between them - or
                // partway along one of them, since `r##"` is four bytes
                // the cursor can be inside.
                assert!(
                    open.start < open.end
                        && open.end <= close.start
                        && close.start < close.end
                        && close.end <= text.len()
                        && open.start <= offset
                        && offset <= close.end,
                    "{text:?} at {offset}: {open:?} and {close:?}"
                );
            }
        }
    }
}
