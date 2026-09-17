// Syntax highlighting and bracket-based indentation, seen the way the
// screen sees them: characters drawn in colours, and text typed with the
// keyboard.
//
// Update snapshots intentionally with:
//   UPDATE_SNAPSHOTS=1 cargo test -p focus-core --test language

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use focus_core::app::{
    App, VcsChange, VcsFile, VcsFileKind, VcsHunk, VcsLine, VcsLineKind, VcsRevisionId,
};
use focus_core::drawing::{DrawCommand, Drawing, FULL_BLOCK};
use focus_core::fuzz::MockIO;
use focus_core::input::{Key, NamedKey};
use focus_core::style::{PAREN_MATCH_COLOR, UNMATCHED_COLOR};
use focus_core::window::WindowId;

mod common;

const ROWS: usize = 16;
const WRAP_CHARS: usize = 100;

// A line of every corner the Rust tokenizer is supposed to know about.
const RUST_SAMPLE: &str = r####"use std::fmt; // a line comment
/* a /* nested */ block comment */
pub fn main<'a>(argv: &'a [&str]) -> Result<(), Error> {
    let raw = r#"a "raw" string"#;
    let bytes = b"bytes\n";
    let ch = '\'';
    let nums = [0xff, 1_000u64, 1.5e-9, 1..2];
    let r#match = argv.len();
    'outer: loop {
        if nums.is_empty() { continue }
        break 'outer;
    }
    fn g<T: ?Sized>(t: &T) -> Result<usize, Error> { Ok(h(t)?) }
    return;
    println!("{raw} {ch} {nums:?} {}", r#match);
}
"####;

// What the screen shows: each row of text, the colour of each of its
// characters as a letter, and a legend saying which colour each letter
// is. Written out in full rather than only where the colours change, so
// that a colour going wrong shows up in the snapshot wherever it is.
fn screen(app: &App, drawing: &Drawing) -> String {
    let [cell_w, cell_h] = [app.cell_size()[0] as f32, app.cell_size()[1] as f32];
    // (row, column) -> (character, colour), so that characters drawn in
    // any order come back in reading order.
    let mut cells: BTreeMap<(usize, usize), (char, [u8; 4])> = BTreeMap::new();
    for command in &drawing.commands {
        let DrawCommand::Character(c) = command else {
            continue;
        };
        // Cursors and gutter bars are fills, not glyphs.
        if c.ch == FULL_BLOCK {
            continue;
        }
        let row = (c.dst.pos[1] / cell_h).round() as usize;
        let col = (c.dst.pos[0] / cell_w).round() as usize;
        cells.insert((row, col), (c.ch, c.color));
    }

    // One letter per distinct colour, in the order they first appear, so
    // the snapshot does not turn over when a colour is retuned.
    let mut legend: Vec<[u8; 4]> = Vec::new();
    let mut letter = |color: [u8; 4]| {
        let ix = legend.iter().position(|c| *c == color).unwrap_or_else(|| {
            legend.push(color);
            legend.len() - 1
        });
        (b'a' + ix as u8) as char
    };

    let mut rows: Vec<(String, String)> = Vec::new();
    for ((row, col), (ch, color)) in cells {
        if rows.len() <= row {
            rows.resize(row + 1, (String::new(), String::new()));
        }
        let (text, colors) = &mut rows[row];
        while text.chars().count() < col {
            text.push(' ');
            colors.push(' ');
        }
        text.push(ch);
        colors.push(letter(color));
    }

    let mut out = String::new();
    for (text, colors) in rows {
        writeln!(out, "{}", text.trim_end()).unwrap();
        writeln!(out, "{}", colors.trim_end()).unwrap();
    }
    out.push('\n');
    for (ix, color) in legend.iter().enumerate() {
        writeln!(
            out,
            "{} = #{:02x}{:02x}{:02x}{:02x}",
            (b'a' + ix as u8) as char,
            color[0],
            color[1],
            color[2],
            color[3]
        )
        .unwrap();
    }
    out
}

fn rust_app(text: &str) -> (App, MockIO, WindowId) {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from("/repo/sample.rs"), text);
    // The file is read from disk on the first tick, not when the buffer is
    // made, so without this there is nothing on screen to colour.
    common::tick(&mut app, &mut io);
    (app, io, window_id)
}

#[test]
fn rust_source_is_colored_by_token() {
    let (mut app, _io, window_id) = rust_app(RUST_SAMPLE);
    let drawing = common::draw(&mut app, window_id, WRAP_CHARS, ROWS);
    check("rust_highlight", &screen(&app, &drawing));
}

// The other three tokenizers, on a line of each corner they have to know
// about: quoting that the generic shapes get wrong, and the words each
// language builds its blocks out of.
#[test]
fn python_source_is_colored_by_token() {
    let text = "\
import os  # a comment
DOC = \"\"\"a docstring
over two lines\"\"\"
def f(raw=r'a\\raw', fmt=f'{os.sep}', num=0x1f_00, flt=1.5e-9):
    for k in raw:
        if not k:
            continue
        break
    else:
        raise ValueError(raw)
    yield raw
    return {'k': [1, 2], **raw}
";
    color_check("python_highlight", "c.py", text);
}

#[test]
fn shell_source_is_colored_by_token() {
    let text = "\
#!/bin/sh
run() {  # not ${#a} and not a#b
  local raw='no \\ escapes here'
  echo \"$1 $(date) `pwd`\" >&2
  case \"$1\" in
  ./build.sh | --verbose)
    exec ./x.sh --flag=1
    ;;
  esac
  while :; do
    break
  done
  return 0
}
run \"$@\" || exit 1
";
    color_check("shell_highlight", "c.sh", text);
}

#[test]
fn nix_source_is_colored_by_token() {
    let text = "\
{ pkgs ? import <nixpkgs> { } }:  # a comment
/* a block comment */
let
  pkg-name = \"focus\";
  script = \'\'
    echo ${pkg-name}
  \'\';
in
if pkgs == null then throw \"no pkgs\" else
pkgs.mkShell { src = ./.; inherit script; buildInputs = [ pkgs.rustc ]; }
";
    color_check("nix_highlight", "c.nix", text);
}

// Markdown is parsed rather than tokenized, and its parts nest: the marks
// show through grey at the edges of what they mark, and a fenced block is
// coloured as whatever language it names.
// Colours only exist with the parser behind them; the reachability
// fixture builds without it, and so can anyone else.
#[cfg(feature = "markdown")]
#[test]
fn markdown_is_colored_by_what_it_marks() {
    let text = "\
# A heading

Some *emphasis*, some **strong**, ~~struck out~~ and `a code span`,
then [a link](https://example.com/x) and <b>raw html</b>.

## Deeper

- a list item
  carrying on
- [ ] a task

> a quote

```rust
fn f() -> usize {
    let x = \"a string\"; // a comment
    x.len()
}
```

```notalanguage
this stays one colour
```
";
    color_check_rows("markdown_highlight", "c.md", text, 30);
}

// Enter lines a new line up with the marker of the line above rather than
// with its text, so that the next `-` can be typed where the last one was.
// The nested item shows it: its lines stay at the indent the writer put it
// at, not two further in under its own text. Nothing ever steps back out:
// a document has no blocks to close, so leaving a list is the writer's to
// do.
#[test]
fn enter_lines_up_with_the_marker_above() {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from("/repo/n.md"), "");
    common::tick(&mut app, &mut io);
    common::text_input(
        &mut app,
        &mut io,
        window_id,
        "# Notes\n\nprose\n\n- an item\n- another\n\n  - nested\nwrapped\n\n1.  numbered\n2.  next\n\n> quoted\n> more\n",
    );
    check("markdown_indent", &common::text(&app));
}

fn color_check(snapshot: &str, name: &str, text: &str) {
    color_check_rows(snapshot, name, text, ROWS);
}

fn color_check_rows(snapshot: &str, name: &str, text: &str, rows: usize) {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from("/repo").join(name), text);
    common::tick(&mut app, &mut io);
    let drawing = common::draw(&mut app, window_id, WRAP_CHARS, rows);
    check(snapshot, &screen(&app, &drawing));
}

#[test]
fn a_file_in_no_known_language_is_left_plain() {
    let (mut app, mut io, window_id) =
        common::file_app(PathBuf::from("/repo/notes.txt"), RUST_SAMPLE);
    common::tick(&mut app, &mut io);
    let drawing = common::draw(&mut app, window_id, WRAP_CHARS, ROWS);
    // The same text with no language behind it: one colour throughout.
    check("plain_highlight", &screen(&app, &drawing));
}

// Typing into a source file re-colours it as it goes: no frame shows the
// colours of the text as it was before the keystroke.
#[test]
fn colors_follow_the_text_as_it_is_typed() {
    let (mut app, mut io, window_id) = rust_app("let x = 1;\nlet y = 2;\n");
    let drawing = common::draw(&mut app, window_id, WRAP_CHARS, ROWS);
    let before = screen(&app, &drawing);
    // The cursor starts at the top of the file, so this comments out the
    // first line and nothing else.
    common::text_input(&mut app, &mut io, window_id, "// ");
    let drawing = common::draw(&mut app, window_id, WRAP_CHARS, ROWS);
    let after = screen(&app, &drawing);
    assert_ne!(before, after);
    check("rust_highlight_after_typing", &after);
}

// Every line of a file, typed in from nothing: the editor puts the indent
// in, so the text typed has none.
#[test]
fn enter_indents_by_bracket_structure() {
    let typed = "\
fn f(x: usize) -> usize {
let y = g(
x,
x + 1,
);
if y > 0 {
return 0;
}
y
}
";
    let (mut app, mut io, window_id) = rust_app("");
    common::text_input(&mut app, &mut io, window_id, typed);
    check("rust_indent", &common::text(&app));
}

// A statement the line above left unfinished carries on one step in from
// where that statement began - and stays there, however many lines it
// runs to.
#[test]
fn enter_continues_an_unfinished_statement() {
    let typed = "\
fn f(items: &[usize]) -> usize {
let x = items
.iter()
.copied()
.sum();
let y = x
+ x
+ x;
g(x
+ y);
y
}
";
    let (mut app, mut io, window_id) = rust_app("");
    common::text_input(&mut app, &mut io, window_id, typed);
    check("rust_indent_continuation", &common::text(&app));
}

// Type a file rustfmt formatted back in with its indentation stripped off,
// and get the same file back. This is the whole indent rule under one
// assertion: chains, run-on expressions, multi-line argument lists and
// patterns, attributes, `let ... else`, and a condition that runs over
// several lines before its block opens.
//
// Blank lines are compared without their trailing whitespace: Enter
// indents the line it makes, and nothing trims a line left empty. Stripping
// trailing whitespace belongs with saving, which is not wired up yet.
#[test]
fn indent_agrees_with_rustfmt() {
    retype("sample.rs", include_str!("fixtures/indent.rs"));
}

// Python's indentation is information, not layout: a dedent is the only
// thing that says a block has ended, so stripping it throws away something
// no rule can put back. What can be asked of the editor is that it does not
// fight: type the file with the indents you mean and nothing moves - which
// is also what checks that `else:`, `elif` and `except:` reindent to the
// right place rather than to a wrong one.
#[test]
fn indent_leaves_python_alone() {
    retype_keeping_indent("sample.py", include_str!("fixtures/indent.py"));
}

// And what it guesses when it has nothing to go on. Every line inherits the
// line above, so a block only ever gets deeper: the dedents are the user's
// to type. Pinned rather than argued about.
#[test]
fn indent_guesses_python_from_nothing() {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from("/repo/g.py"), "");
    common::tick(&mut app, &mut io);
    common::text_input(
        &mut app,
        &mut io,
        window_id,
        "def f(x):\nif x:\nreturn [\n1,\n]\nelse:\nreturn 0\n",
    );
    check("python_indent_guess", &common::text(&app));
}

#[test]
fn indent_reproduces_shell() {
    retype("sample.sh", include_str!("fixtures/indent.sh"));
}

#[test]
fn indent_reproduces_nix() {
    retype("sample.nix", include_str!("fixtures/indent.nix"));
}

// Ctrl+tab over a selection puts every line where the rules say it goes.
// The same three files the retyping tests write out from nothing, with
// every indent stripped off and put back in one keystroke instead: if the
// rules determine the indent at all, these are the files they determine.
#[test]
fn reformat_restores_rust() {
    reformat("sample.rs", include_str!("fixtures/indent.rs"));
}

// Nix is the one of the three with a string that runs over lines, and
// that is the one place reformatting differs from typing the file in:
// what is inside a string is text, so the spaces at the front of it are
// part of what it says and are left exactly where they are. Everything
// outside one comes back.
#[test]
fn reformat_restores_nix_outside_its_strings() {
    let stripped = strip_indent(include_str!("fixtures/indent.nix"));
    check("reformat_nix", &reformatted("sample.nix", &stripped));
}

#[test]
fn reformat_restores_shell() {
    reformat("sample.sh", include_str!("fixtures/indent.sh"));
}

// Python's indent is the program rather than its layout, so there is
// nothing to work it out from and ctrl+tab leaves it alone. A document is
// the same: the indent is the writer's.
#[test]
fn reformat_leaves_python_alone() {
    let stripped = strip_indent(include_str!("fixtures/indent.py"));
    assert_eq!(reformatted("sample.py", &stripped), stripped);
}

#[test]
fn reformat_leaves_markdown_alone() {
    let stripped = strip_indent(include_str!("fixtures/sample.md"));
    assert_eq!(reformatted("notes.md", &stripped), stripped);
}

fn strip_indent(formatted: &str) -> String {
    formatted
        .lines()
        .map(|line| format!("{}\n", line.trim_start()))
        .collect()
}

/// Open `text` as `name`, select all of it, and press ctrl+tab.
fn reformatted(name: &str, text: &str) -> String {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from("/repo").join(name), text);
    common::tick(&mut app, &mut io);
    common::alt_key(&mut app, &mut io, window_id, Key::Character("i"));
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Space));
    common::alt_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Tab));
    app.assert_invariants();
    common::text(&app)
}

/// Strip every indent off `formatted`, reformat the lot, and require the
/// file back exactly.
fn reformat(name: &str, formatted: &str) {
    assert_eq!(reformatted(name, &strip_indent(formatted)), formatted);
}

/// Type `formatted` into a new file called `name`, with every line's
/// indentation stripped off, and require the file back exactly.
///
/// Blank lines are compared without their trailing whitespace: Enter
/// indents the line it makes, and nothing trims a line left empty.
/// Stripping trailing whitespace belongs with saving, which is not wired
/// up yet.
fn retype(name: &str, formatted: &str) {
    let stripped: String = formatted
        .lines()
        .map(|line| format!("{}\n", line.trim_start()))
        .collect();

    let (mut app, mut io, window_id) = common::file_app(PathBuf::from("/repo").join(name), "");
    common::tick(&mut app, &mut io);
    common::text_input(&mut app, &mut io, window_id, &stripped);

    let typed = common::text(&app);
    let typed: Vec<&str> = typed.lines().map(|line| line.trim_end()).collect();
    let expected: Vec<&str> = formatted.lines().collect();
    assert_eq!(typed.join("\n"), expected.join("\n"));
}

// Where the rule and rustfmt part company, pinned so that it is visible
// rather than folklore. Both cases are rustfmt indenting from the
// expression tree, which a rule that knows only about brackets and how
// lines end cannot see: rustfmt steps a chain in again when it hangs off a
// line that is itself a continuation, and puts `where` back at the item it
// belongs to rather than one step into it. Everything else here agrees.
#[test]
fn indent_differs_from_rustfmt_here() {
    let typed = "\
fn f(a: bool) -> usize
where
usize: Copy,
{
if a
&& some_receiver
.method_call()
.other_method()
{
return 1;
}
0
}
";
    let (mut app, mut io, window_id) = rust_app("");
    common::text_input(&mut app, &mut io, window_id, typed);
    check("rust_indent_divergence", &common::text(&app));
}

// A generated buffer is coloured by the page that writes it, not by a
// tokenizer: the diff page's added and removed lines carry the same
// colours as the change bars in an edit page's gutter.
#[test]
fn a_diff_page_colors_its_own_lines() {
    let (mut app, mut io, window_id) =
        common::file_app(PathBuf::from("/repo/file.txt"), "one\nTWO\nthree\n");
    io.repo_roots.push(PathBuf::from("/repo"));
    io.vcs_changes.insert(
        (PathBuf::from("/repo"), VcsRevisionId::WorkingCopy),
        VcsChange {
            root: PathBuf::from("/repo"),
            change_id: "qpvuntsmwlqt".into(),
            commit_id: "1f2a3b4c5d6e".into(),
            author: "Jamie (2026-09-14 15:30:00)".into(),
            description: "a change".into(),
            files: vec![VcsFile {
                relative_path: PathBuf::from("file.txt"),
                kind: VcsFileKind::Modified,
                binary: false,
                hunks: vec![VcsHunk {
                    old_lines: 0..3,
                    new_lines: 0..3,
                    lines: vec![
                        VcsLine {
                            kind: VcsLineKind::Context,
                            text: "one".into(),
                        },
                        VcsLine {
                            kind: VcsLineKind::Removed,
                            text: "two".into(),
                        },
                        VcsLine {
                            kind: VcsLineKind::Added,
                            text: "TWO".into(),
                        },
                        VcsLine {
                            kind: VcsLineKind::Context,
                            text: "three".into(),
                        },
                    ],
                }],
            }],
        },
    );
    common::tick(&mut app, &mut io);
    // ctrl+2 opens the diff page over the file.
    common::control_key(&mut app, &mut io, window_id, Key::Character("2"));
    common::tick(&mut app, &mut io);
    let drawing = common::draw(&mut app, window_id, WRAP_CHARS, ROWS);
    check("diff_highlight", &screen(&app, &drawing));
}

/// Type `formatted` into a new file called `name` line by line, replacing
/// whatever Enter guessed with the indent the line actually has, and
/// require the file back exactly. Nothing the editor does as the rest of
/// the line is typed may move it again.
fn retype_keeping_indent(name: &str, formatted: &str) {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from("/repo").join(name), "");
    common::tick(&mut app, &mut io);

    for (ix, line) in formatted.lines().enumerate() {
        if ix > 0 {
            common::key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
        }
        let wanted = line.len() - line.trim_start().len();
        let text = common::text(&app);
        let guessed = text.len() - text.trim_end_matches(' ').len();
        for _ in 0..guessed.saturating_sub(wanted) {
            common::key(
                &mut app,
                &mut io,
                window_id,
                Key::Named(NamedKey::Backspace),
            );
        }
        for _ in 0..wanted.saturating_sub(guessed) {
            common::key(&mut app, &mut io, window_id, Key::Named(NamedKey::Space));
        }
        common::text_input(&mut app, &mut io, window_id, line.trim_start());
    }

    let typed = common::text(&app);
    let typed: Vec<&str> = typed.lines().map(|line| line.trim_end()).collect();
    let expected: Vec<&str> = formatted.lines().collect();
    assert_eq!(typed.join("\n"), expected.join("\n"));
}

fn check(name: &str, actual: &str) {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        expected, actual,
        "{name} changed; rerun with UPDATE_SNAPSHOTS=1 to accept"
    );
}

// `G` marks a character drawn in the colour of the pair around the
// cursor, `R` one drawn in the colour of a token with nothing to pair
// with - which is the same wherever the cursor is.
fn pair_screen(app: &App, drawing: &Drawing) -> String {
    let [cell_w, cell_h] = [app.cell_size()[0] as f32, app.cell_size()[1] as f32];
    let mut cells: BTreeMap<(usize, usize), char> = BTreeMap::new();
    let mut marks: BTreeMap<(usize, usize), char> = BTreeMap::new();
    for command in &drawing.commands {
        let DrawCommand::Character(c) = command else {
            continue;
        };
        // Cursors, gutter bars and the scrollbar are fills, not glyphs.
        if c.ch == FULL_BLOCK {
            continue;
        }
        let row = (c.dst.pos[1] / cell_h).round() as usize;
        let col = (c.dst.pos[0] / cell_w).round() as usize;
        cells.insert((row, col), c.ch);
        match c.color {
            PAREN_MATCH_COLOR => marks.insert((row, col), 'G'),
            UNMATCHED_COLOR => marks.insert((row, col), 'R'),
            _ => None,
        };
    }

    let rows = cells.keys().chain(marks.keys()).map(|(row, _)| *row).max();
    let mut out = String::new();
    for row in 0..=rows.unwrap_or(0) {
        let width = cells
            .range((row, 0)..(row + 1, 0))
            .chain(marks.range((row, 0)..(row + 1, 0)))
            .map(|((_, col), _)| *col + 1)
            .max()
            .unwrap_or(0);
        let line: String = (0..width)
            .map(|col| cells.get(&(row, col)).copied().unwrap_or(' '))
            .collect();
        let mark: String = (0..width)
            .map(|col| marks.get(&(row, col)).copied().unwrap_or(' '))
            .collect();
        if line.trim().is_empty() && mark.trim().is_empty() {
            continue;
        }
        writeln!(out, "{}", line.trim_end()).unwrap();
        writeln!(out, "{}", mark.trim_end()).unwrap();
    }
    out
}

// `|` in the sample says where the cursor goes; it is taken out of the
// text before the file is opened.
fn pair_case(out: &mut String, name: &str, sample: &str) {
    for line in sample.lines() {
        writeln!(out, "### {line}").unwrap();
    }
    let offset = sample.find('|').expect("no cursor in the sample");
    let text = sample.replacen('|', "", 1);
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from("/repo").join(name), &text);
    common::tick(&mut app, &mut io);
    common::alt_key(&mut app, &mut io, window_id, Key::Character("i"));
    for _ in 0..offset {
        common::control_key(&mut app, &mut io, window_id, Key::Character("l"));
    }
    // So that the status bar at the foot of the snapshot says where the
    // cursor ended up, as a check on the case having done what it says.
    common::tick(&mut app, &mut io);
    let drawing = common::draw(&mut app, window_id, WRAP_CHARS, ROWS);
    out.push_str(&pair_screen(&app, &drawing));
    out.push('\n');
}

// Where the cursor is, what is around it. The cursor sits between
// characters, so a bracket it is only next to is not one it is inside.
// The red of a token with nothing to pair with is in here too, to show
// that it is the same red whether the cursor is by it or not.
#[test]
fn the_pair_around_the_cursor_is_highlighted() {
    let mut out = String::new();
    for sample in [
        // Inside the pair, either end of it, and outside it.
        "fn f() { foo(ba|r, baz) }",
        "fn f() { foo(|bar, baz) }",
        "fn f() { foo(bar, baz|) }",
        "fn f() { foo|(bar, baz) }",
        "fn f() { foo(bar, baz)| }",
        // The innermost pair, and the one outside it once the cursor has
        // stepped out of the inner one.
        "fn f() { outer(inner(|x)) }",
        "fn f() { outer(inner(x)|) }",
        // A `(` that meets a `]` is two brackets that pair with nothing,
        // red on their own account - the same red with the cursor
        // somewhere else entirely.
        "fn f() { foo(ba|r] }",
        "fn f()| { foo(bar] }",
        // A stray bracket that closed nothing is not what the cursor is
        // in: the pair around it is the one that does pair up.
        "fn f() { foo(a] b|c) }",
        // The `[` in the middle is the typo, and the only token drawn
        // as one: the `(` and `)` either side of it pair up over the top
        // of it, and so do the `[` and `]` outside those.
        "fn f() { vec![0u8; (w * [ h|) as usize]; }",
        // One end with nothing to pair with at all: red, and the pair
        // around the cursor is whatever does pair up.
        "fn f() { foo(ba|r }",
        "fn f() { ba|r) }",
        // A comment is one token, so the brackets written in it are not
        // brackets: the pair around the cursor is the one around the
        // comment.
        "fn f() {\n    // no|te (here\n}",
        // The quotes of the string the cursor is in, however long they
        // are, and the opening one on its own while the string is still
        // being typed.
        "fn f() { let s = \"he|llo\"; }",
        "fn f() { let s = r#\"he|llo\"#; }",
        "fn f() { let s = \"he|llo }",
        // A bracket inside a string is part of the string, not a bracket.
        "fn f() { let s = \"(he|llo\"; }",
        // Angle brackets, when they are brackets.
        "fn f() { let v: Vec<Hash|Map<K, V>> = x; }",
        "fn f() { let v: Vec<HashMap<K|, V>> = x; }",
        "fn f() { foo::<T|>(); }",
        // Spaced, so comparisons: the pair around the cursor is the
        // block, not the `<` and `>` either side of it.
        "fn f() { let b = a < b |&& c > d; }",
    ] {
        pair_case(&mut out, "sample.rs", sample);
    }
    for sample in [
        "x = f\"\"\"he|llo\"\"\"\n",
        "def f():\n    return [1, |2]\n",
    ] {
        pair_case(&mut out, "sample.py", sample);
    }
    // Nix's `let ... in` is a bracket pair made of words.
    pair_case(&mut out, "sample.nix", "let x = |1; in x\n");
    // Every shell `case` pattern ends with a `)` that never had a `(`, so
    // it is not a bracket around anything.
    pair_case(&mut out, "sample.sh", "case $x in\n  a) fo|o;;\nesac\n");
    check("pair_highlight", &out);
}
