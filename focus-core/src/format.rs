//! Reformatting a whole file on save, for the languages that have a
//! formatter worth running.
//!
//! A formatter has to be a real one - the editor is not the place to
//! invent a house style. Two of them are libraries, and run on the text
//! in memory: nixfmt for Nix and ruff for Python. Rust's is a program.
//!
//! rustfmt is not published as a crate anyone can link: `rustfmt-nightly`
//! was last published in September 2020 against `rustc-ap-rustc_ast` 677,
//! a snapshot of the compiler's own parser whose first ten lines ask for
//! `box_syntax`, `const_fn` and `try_trait` - features the language has
//! since removed, so no compiler released in years can build it, and
//! everything newer lives in the rustfmt repository and is built against
//! the exact compiler it ships with. prettyplease is a crate and does
//! build, but it prints a `syn` tree, and `syn` parses code: `///` and
//! `//!` survive as the attributes they are and every ordinary `//` is
//! gone before the tree exists. On the source of this editor that is 161
//! of the 215 comment lines in `editor.rs`. So Rust is formatted by
//! running `rustfmt`, which keeps them, and which is not there to run on
//! a machine that has not installed it - in which case the file is saved
//! the way it was typed.
//!
//! Shell has nothing mature. Markdown has several, but a document's
//! layout is the writer's, which is why `ctrl+tab` leaves it alone too.

use std::path::Path;

use bstr::{BStr, BString};

use crate::app::IO;
use crate::language::Language;

/// The whole file, reformatted - or None to leave it exactly as it is.
///
/// None covers every way this can decline: a language with no formatter,
/// text that is not UTF-8, and text that does not parse. That last one is
/// the important one. A file is saved far more often half-written than
/// finished, and a formatter handed code that is not code yet would at
/// best refuse and at worst rearrange it; the text is written to disk the
/// way it was typed instead.
pub(crate) fn format(
    io: &mut dyn IO,
    language: Option<Language>,
    dir: &Path,
    text: &BStr,
) -> Option<BString> {
    match language? {
        Language::Nix => nix(text),
        Language::Python => python(text),
        Language::Rust => rust(io, dir, text),
        Language::Shell | Language::Markdown => None,
    }
}

/// nixfmt, which is what `nix fmt` runs and what RFC 166 asks for.
#[cfg(feature = "nix-fmt")]
fn nix(text: &BStr) -> Option<BString> {
    use bstr::ByteSlice;

    Some(BString::from(nixfmt_rs::format(text.to_str().ok()?).ok()?))
}

#[cfg(not(feature = "nix-fmt"))]
fn nix(_text: &BStr) -> Option<BString> {
    None
}

/// Ruff's formatter, which is what `ruff format` runs.
#[cfg(feature = "python-fmt")]
fn python(text: &BStr) -> Option<BString> {
    use bstr::ByteSlice;

    let formatted = ruff_python_formatter::format_module_source(
        text.to_str().ok()?,
        ruff_python_formatter::PyFormatOptions::default(),
    )
    .ok()?;
    Some(BString::from(formatted.as_code()))
}

#[cfg(not(feature = "python-fmt"))]
fn python(_text: &BStr) -> Option<BString> {
    None
}

/// rustfmt, reading the file on stdin and writing it back on stdout.
fn rust(io: &mut dyn IO, dir: &Path, text: &BStr) -> Option<BString> {
    // Run where the file is, so that a `rustfmt.toml` beside it is the
    // one that applies. The edition is not usually in that file - cargo
    // is what tells rustfmt which edition a crate is - and rustfmt's own
    // default is 2015, which does not parse most code written since.
    let formatted = io
        .process_run(
            dir,
            BStr::new("rustfmt"),
            &[BStr::new("--edition"), BStr::new("2024")],
            text,
        )
        .ok()?;
    // A formatter that returns nothing for something is not formatting,
    // whatever it exited with: the file is not going to be emptied on the
    // strength of it.
    if formatted.is_empty() && !text.is_empty() {
        return None;
    }
    Some(BString::from(formatted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fuzz::MockIO;

    /// The languages below are formatted in memory, so nothing reaches
    /// this - but `format` takes an IO to run rustfmt with.
    fn formatted(language: Option<Language>, text: &BStr) -> Option<BString> {
        format(&mut MockIO::new(), language, Path::new("/"), text)
    }

    #[cfg(feature = "nix-fmt")]
    #[test]
    fn nix_is_formatted() {
        let text = BStr::new("{foo=1;bar   =   [1 2];}");
        assert_eq!(
            formatted(Some(Language::Nix), text).unwrap(),
            "{\n  foo = 1;\n  bar = [\n    1\n    2\n  ];\n}\n"
        );
    }

    /// A file being saved is often half-written, and this is what that
    /// looks like: the text goes to disk as it was typed.
    #[cfg(feature = "nix-fmt")]
    #[test]
    fn nix_that_does_not_parse_is_left_alone() {
        assert_eq!(formatted(Some(Language::Nix), BStr::new("{ foo = ")), None);
    }

    /// Every other language, and a file in none.
    #[test]
    fn languages_with_no_formatter_are_left_alone() {
        let text = BStr::new("echo  hello");
        assert_eq!(formatted(Some(Language::Shell), text), None);
        assert_eq!(formatted(Some(Language::Markdown), text), None);
        assert_eq!(formatted(None, text), None);
    }

    #[cfg(feature = "python-fmt")]
    #[test]
    fn python_is_formatted() {
        let text = BStr::new("def f(x,y):\n  return {  'a':x }\n");
        assert_eq!(
            formatted(Some(Language::Python), text).unwrap(),
            "def f(x, y):\n    return {\"a\": x}\n"
        );
    }

    #[cfg(feature = "python-fmt")]
    #[test]
    fn python_that_does_not_parse_is_left_alone() {
        assert_eq!(formatted(Some(Language::Python), BStr::new("def f(")), None);
    }
}
