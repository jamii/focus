//! Markdown, parsed by markdown-rs.
//!
//! Every other language here is tokenized: a flat run of tokens covering
//! the text, which is what indentation and bracket matching are built on.
//! A document is not shaped like that. Its parts nest - a link inside a
//! bold run inside a list item - and the marks themselves are not in the
//! tree at all, only the gaps between what is marked. So markdown is
//! painted rather than tokenized: each part colours the whole of its own
//! range, and whatever sits inside it paints over the top. What is left
//! showing at the edges is the `#`, the `**`, the `[` and `]`, which is
//! exactly the part that should read as punctuation.

use std::ops::Range;

use bstr::{BStr, ByteSlice};
use markdown::ParseOptions;
use markdown::mdast::Node;

use super::{Highlight, Language, Span};
use crate::style::{
    COMMENT_COLOR, MARKUP_CODE_COLOR, MARKUP_EMPHASIS_COLOR, MARKUP_STRONG_COLOR, TEXT_COLOR,
    heading_color,
};

/// The colour of every byte that has one, as spans in order.
pub(super) fn spans(text: &BStr) -> Vec<Span> {
    // A parser needs text, and a buffer is bytes: a file that is not UTF-8
    // is not markdown, whatever it is called, so it goes uncoloured.
    let Ok(source) = text.to_str() else {
        return Vec::new();
    };
    // GFM, because tables, task lists and `~~struck out~~` are what people
    // write. The parse only fails on the extensions that can hold real
    // syntax errors - MDX - which are not on.
    let Ok(root) = markdown::to_mdast(source, &ParseOptions::gfm()) else {
        return Vec::new();
    };

    // One colour per byte, painted over as the walk goes inwards. Far more
    // memory than the spans it turns into, and the only thing that makes
    // nested parts fall out right without interval arithmetic. Documents
    // are small; if that stops being true this wants to be a stack of
    // ranges instead.
    let mut paint: Vec<Option<[u8; 4]>> = vec![None; text.len()];
    // A stack rather than recursion, so that a document nested a thousand
    // lists deep cannot take the editor with it. A node is always painted
    // before anything inside it is reached, and things at the same level
    // do not overlap, so the order within a level does not matter.
    let mut todo = vec![(&root, TEXT_COLOR)];
    while let Some((node, inherited)) = todo.pop() {
        let Some(position) = node.position() else {
            continue;
        };
        let range = position.start.offset..position.end.offset;
        if range.end > text.len() {
            continue;
        }

        // The whole part, marks included, in the colour its marks should
        // be. Everything inside paints over it.
        if let Some(color) = mark_color(node) {
            fill(&mut paint, range.clone(), color);
        }
        // What the words inside should be: a heading's words are heading
        // coloured, a link's words are just words.
        let body = body_color(node).unwrap_or(inherited);

        match node {
            // The leaves that are words rather than structure.
            Node::Text(_) => fill(&mut paint, range, body),
            Node::InlineCode(_) | Node::InlineMath(_) | Node::Math(_) => {
                fill(&mut paint, range, MARKUP_CODE_COLOR)
            }
            Node::Code(code) => {
                // A fenced block holds no child nodes, only its text, so
                // its content is found rather than walked to.
                if let Some(content) = content_of(text, &range, &code.value) {
                    fill(&mut paint, content.clone(), MARKUP_CODE_COLOR);
                    if let Some(language) = code.lang.as_deref().and_then(fence_language) {
                        paint_as(&mut paint, text, content, language);
                    }
                }
            }
            _ => {}
        }

        for child in node.children().into_iter().flatten() {
            todo.push((child, body));
        }
    }

    runs(&paint)
}

/// What the marks around a part are drawn in. None leaves whatever is
/// underneath: a paragraph has no marks of its own to show.
fn mark_color(node: &Node) -> Option<[u8; 4]> {
    match node {
        Node::Root(_) | Node::Paragraph(_) | Node::Text(_) => None,
        // Everything else shows its marks in the same grey, and whatever
        // it holds paints over them.
        _ => Some(COMMENT_COLOR),
    }
}

/// What the words inside a part are drawn in. None keeps the colour of
/// whatever the part is inside.
fn body_color(node: &Node) -> Option<[u8; 4]> {
    match node {
        Node::Heading(heading) => Some(heading_color(heading.depth as usize)),
        Node::Emphasis(_) => Some(MARKUP_EMPHASIS_COLOR),
        Node::Strong(_) => Some(MARKUP_STRONG_COLOR),
        Node::Delete(_) => Some(COMMENT_COLOR),
        // A link's words are what the sentence says; only the brackets and
        // the URL around them are marks.
        Node::Link(_) | Node::LinkReference(_) | Node::Image(_) | Node::ImageReference(_) => {
            Some(TEXT_COLOR)
        }
        _ => None,
    }
}

/// Where a code block's own text sits inside the fence around it. The
/// parser hands over the code but not where it came from, so it is found
/// by looking: past the opening fence, and exactly as long as the code
/// itself. None if that does not come out matching, which leaves the
/// block coloured as one thing rather than coloured wrong.
fn content_of(text: &BStr, block: &Range<usize>, value: &str) -> Option<Range<usize>> {
    let start = match text[block.clone()].find_byte(b'\n') {
        // Fenced: the code starts on the line after the fence.
        Some(newline) => block.start + newline + 1,
        // One line and no newline in it: an empty fenced block.
        None => return None,
    };
    let content = start..start + value.len();
    (content.end <= block.end && text[content.clone()] == *value.as_bytes()).then_some(content)
}

/// The language a fence names, if it names one we can read.
fn fence_language(info: &str) -> Option<Language> {
    // `rust,ignore` and `rust title=x` both name Rust.
    let name = info.split([',', ' ']).next()?.trim();
    match name {
        "rust" | "rs" => Some(Language::Rust),
        "python" | "py" => Some(Language::Python),
        "sh" | "bash" | "shell" | "console" => Some(Language::Shell),
        "nix" => Some(Language::Nix),
        // Markdown inside markdown terminates: what is inside a fence is
        // always shorter than what holds it.
        "markdown" | "md" => Some(Language::Markdown),
        _ => None,
    }
}

/// Paint `code` with `language`'s own colours, as though that stretch of
/// the document were a file of its own.
fn paint_as(paint: &mut [Option<[u8; 4]>], text: &BStr, code: Range<usize>, language: Language) {
    let source = &text[code.clone()];
    let highlight = Highlight::new(Some(language), source);
    let mut colors = Vec::new();
    highlight.color_runs(source, 0..source.len(), &mut colors);
    for (range, color) in colors {
        fill(
            paint,
            code.start + range.start..code.start + range.end,
            color,
        );
    }
}

fn fill(paint: &mut [Option<[u8; 4]>], range: Range<usize>, color: [u8; 4]) {
    for byte in &mut paint[range] {
        *byte = Some(color);
    }
}

/// The painted bytes as the longest runs of one colour, which is what a
/// `Span` is. Bytes with no colour of their own are left out.
fn runs(paint: &[Option<[u8; 4]>]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for (offset, color) in paint.iter().enumerate() {
        let Some(color) = *color else { continue };
        match spans.last_mut() {
            Some(last) if last.range.end == offset && last.color == color => last.range.end += 1,
            _ => spans.push(Span {
                range: offset..offset + 1,
                color,
            }),
        }
    }
    spans
}

/// Where a new line under the one holding `offset` belongs: alongside what
/// that line is saying, which is past any list or quote marker on it. A
/// document has no blocks to close, so nothing ever steps back out - that
/// is the writer's to do.
pub(super) fn content_indent(text: &BStr, offset: usize) -> usize {
    let start = text[..offset].rfind_byte(b'\n').map_or(0, |ix| ix + 1);
    let line = &text[start..];
    let mut pos = line.iter().take_while(|byte| **byte == b' ').count();

    // A line with nothing on it ends the block above, so the next line
    // starts over at the left. The spaces Enter left on it are not
    // content - nothing has been written there to line up with.
    if matches!(line.get(pos), None | Some(b'\n')) {
        return 0;
    }

    // `- `, `* `, `+ `, `> `, `1. `, `1) `: a marker, and then the space
    // after it that puts the content where it starts.
    let marker = match line.get(pos) {
        Some(b'-' | b'*' | b'+' | b'>') => 1,
        Some(byte) if byte.is_ascii_digit() => {
            let digits = line[pos..]
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            match line.get(pos + digits) {
                Some(b'.' | b')') => digits + 1,
                _ => return pos,
            }
        }
        _ => return pos,
    };
    if line.get(pos + marker) != Some(&b' ') {
        return pos;
    }
    pos += marker;
    // Everything up to the content, however much space was left after the
    // marker, so that `-   foo` lines up under the `foo`.
    pos + line[pos..].iter().take_while(|byte| **byte == b' ').count()
}
