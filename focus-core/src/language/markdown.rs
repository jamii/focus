//! Markdown, parsed by pulldown-cmark.
//!
//! Every other language here is tokenized: a flat run of tokens covering
//! the text, which is what indentation and bracket matching are built on.
//! A document is not shaped like that. Its parts nest and overlap - a link
//! inside a bold run inside a list item - and the marks themselves are not
//! events at all, only the gaps between what is marked. So markdown is
//! painted rather than tokenized: each part colours the whole of its own
//! range, and whatever sits inside it paints over the top. What is left
//! showing at the edges is the `#`, the `**`, the `[` and `]`, which is
//! exactly the part that should read as punctuation.

use bstr::{BStr, ByteSlice};

use super::Span;

/// The colour of every byte that has one, as spans in order.
#[cfg(feature = "markdown")]
pub(super) fn spans(text: &BStr) -> Vec<Span> {
    parse::spans(text)
}

/// Built without the `markdown` feature there is no parser, so a document
/// gets no colours. Everything else about it - that it is a document at
/// all, where Enter puts the next line - is unchanged.
#[cfg(not(feature = "markdown"))]
pub(super) fn spans(_text: &BStr) -> Vec<Span> {
    Vec::new()
}

#[cfg(feature = "markdown")]
mod parse {
    use std::ops::Range;

    use bstr::{BStr, ByteSlice};
    use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

    use crate::language::{Highlight, Language, Span};
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

        // One colour per byte, painted over as the parse nests inwards. Far
        // more memory than the spans it turns into, and the only thing that
        // makes overlapping parts fall out right without interval arithmetic.
        // Documents are small; if that stops being true this wants to be a
        // stack of ranges instead.
        let mut paint: Vec<Option<[u8; 4]>> = vec![None; source.len()];
        // What plain text inside the part being read should be coloured. A
        // heading's words are heading-coloured, a link's words are not.
        let mut body = vec![TEXT_COLOR];
        // The language of the fenced code block being read, and how far its
        // code has run so far, so that it can be highlighted as its own
        // language once its end is known.
        let mut fence: Option<(Option<Language>, Option<Range<usize>>)> = None;

        let options = Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TABLES
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_FOOTNOTES;
        for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
            match event {
                Event::Start(tag) => {
                    // The whole part, marks included, in the colour its marks
                    // should be. Everything inside paints over it.
                    if let Some(color) = mark_color(&tag) {
                        fill(&mut paint, range, color);
                    }
                    if let Tag::CodeBlock(kind) = &tag {
                        fence = Some((fence_language(kind), None));
                    }
                    body.push(body_color(&tag).unwrap_or(*body.last().expect("never empty")));
                }
                Event::End(end) => {
                    if end == TagEnd::CodeBlock
                        && let Some((Some(language), Some(code))) = fence.take()
                    {
                        paint_as(&mut paint, text, code, language);
                    }
                    // The push in `Start` and this pop are the only two, so
                    // the stack cannot run dry.
                    body.pop();
                }
                Event::Text(_) => {
                    // Inside a fence the text is code, and its extent is
                    // gathered up to be highlighted once the fence ends.
                    if let Some((_, code)) = &mut fence {
                        *code = Some(match code {
                            Some(code) => code.start..range.end,
                            None => range.clone(),
                        });
                    }
                    fill(&mut paint, range, *body.last().expect("never empty"));
                }
                // A code span is all one thing, backticks included.
                Event::Code(_) | Event::InlineMath(_) | Event::DisplayMath(_) => {
                    fill(&mut paint, range, MARKUP_CODE_COLOR)
                }
                // Raw HTML, a horizontal rule, a checkbox, a `\` at the end of
                // a line: marks rather than words.
                Event::Html(_)
                | Event::InlineHtml(_)
                | Event::Rule
                | Event::TaskListMarker(_)
                | Event::HardBreak
                | Event::FootnoteReference(_) => fill(&mut paint, range, COMMENT_COLOR),
                Event::SoftBreak => {}
            }
        }

        runs(&paint)
    }

    /// What the marks around a part are drawn in. None leaves whatever is
    /// underneath: a paragraph has no marks of its own to show.
    fn mark_color(tag: &Tag) -> Option<[u8; 4]> {
        match tag {
            Tag::Paragraph | Tag::MetadataBlock(_) => None,
            // Everything else shows its marks in the same grey, and whatever
            // it holds paints over them.
            _ => Some(COMMENT_COLOR),
        }
    }

    /// What the words inside a part are drawn in. None keeps the colour of
    /// whatever the part is inside.
    fn body_color(tag: &Tag) -> Option<[u8; 4]> {
        match tag {
            Tag::Heading { level, .. } => Some(heading_color(*level as usize)),
            Tag::Emphasis => Some(MARKUP_EMPHASIS_COLOR),
            Tag::Strong => Some(MARKUP_STRONG_COLOR),
            Tag::Strikethrough => Some(COMMENT_COLOR),
            Tag::CodeBlock(_) => Some(MARKUP_CODE_COLOR),
            // A link's words are what the sentence says; only the brackets and
            // the URL around them are marks.
            Tag::Link { .. } | Tag::Image { .. } => Some(TEXT_COLOR),
            _ => None,
        }
    }

    /// The language a fence names, if it names one we can read.
    fn fence_language(kind: &CodeBlockKind) -> Option<Language> {
        let CodeBlockKind::Fenced(info) = kind else {
            return None;
        };
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
    fn paint_as(
        paint: &mut [Option<[u8; 4]>],
        text: &BStr,
        code: Range<usize>,
        language: Language,
    ) {
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
                Some(last) if last.range.end == offset && last.color == color => {
                    last.range.end += 1
                }
                _ => spans.push(Span {
                    range: offset..offset + 1,
                    color,
                }),
            }
        }
        spans
    }
}

/// Where a new line under the one holding `offset` belongs: at the same
/// indent as that line. A list item's marker sits at that indent, so the
/// new line starts where the next `-` goes rather than where the item's
/// text does - what starts the next item is typing its marker, and a line
/// that carries the item on instead is two spaces the writer adds. A
/// document has no blocks to close, so nothing ever steps back out -
/// leaving a list is the writer's to do.
pub(super) fn next_line_indent(text: &BStr, offset: usize) -> usize {
    let start = text[..offset].rfind_byte(b'\n').map_or(0, |ix| ix + 1);
    let line = &text[start..];
    let indent = line.iter().take_while(|byte| **byte == b' ').count();

    // A line with nothing on it ends the block above, so the next line
    // starts over at the left. The spaces Enter left on it are not
    // content - nothing has been written there to line up with.
    if matches!(line.get(indent), None | Some(b'\n')) {
        return 0;
    }

    indent
}
