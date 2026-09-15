// The jj integration, end to end over a real repo: init a workspace, record
// a base commit, change the working copy, and read back what the editor
// would show.
//
// The timing tests at the bottom are `#[ignore]`d - they measure rather than
// assert. Run them with:
//   cargo test -p focus --test vcs -- --ignored --nocapture

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bstr::ByteSlice as _;
use focus::vcs::{Poller, Vcs};
use focus_core::app::{VcsChange, VcsFileKind, VcsLineKind, VcsRevision, VcsRevisionId};
use jj_lib::config::StackedConfig;
use jj_lib::gitignore::GitIgnoreFile;
use jj_lib::matchers::{EverythingMatcher, NothingMatcher};
use jj_lib::repo::Repo as _;
use jj_lib::settings::UserSettings;
use jj_lib::working_copy::SnapshotOptions;
use jj_lib::workspace::Workspace;
use pollster::block_on;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("focus-vcs-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

fn settings() -> UserSettings {
    UserSettings::from_config(StackedConfig::with_defaults()).unwrap()
}

// A jj repo whose parent commit holds `files`, and whose working copy is a
// new, so-far-empty change on top of it. This is the setup an editor sees:
// `@` is the change you are making, `@-` is what you are changing.
fn repo_with_base(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = dir(name);
    let settings = settings();
    let (mut workspace, repo) = block_on(Workspace::init_simple(&settings, &root)).unwrap();

    for (path, contents) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    // Record the files as a commit, and start a new change on top of it.
    // This is the one place we do what a jj command does - the editor only
    // ever reads (see the comment at the top of focus/src/vcs.rs).
    let name = workspace.workspace_name().to_owned();
    let wc_id = repo.view().get_wc_commit_id(&name).unwrap().clone();
    let wc_commit = repo.store().get_commit(&wc_id).unwrap();

    let mut locked_ws = block_on(workspace.start_working_copy_mutation()).unwrap();
    let options = SnapshotOptions {
        base_ignores: GitIgnoreFile::empty(),
        progress: None,
        start_tracking_matcher: &EverythingMatcher,
        force_tracking_matcher: &NothingMatcher,
        max_new_file_size: 1024 * 1024,
    };
    let (tree, _stats) = block_on(locked_ws.locked_wc().snapshot(&options)).unwrap();

    let mut tx = repo.start_transaction();
    let mut_repo = tx.repo_mut();
    let base = block_on(
        mut_repo
            .rewrite_commit(&wc_commit)
            .set_tree(tree.clone())
            .set_description("base")
            .write(),
    )
    .unwrap();
    // The rewrite above leaves the old working-copy commit's descendants
    // (there are none) to be rebased; the transaction refuses to commit
    // until that is acknowledged.
    block_on(mut_repo.rebase_descendants()).unwrap();
    let new_wc = block_on(
        mut_repo
            .new_commit(vec![base.id().clone()], tree)
            .set_description("the change under test")
            .write(),
    )
    .unwrap();
    mut_repo
        .set_wc_commit(name.clone(), new_wc.id().clone())
        .unwrap();
    let repo = block_on(tx.commit("test setup")).unwrap();
    block_on(locked_ws.finish(repo.op_id().clone())).unwrap();

    root
}

// The change, without the parts that differ from run to run (ids, author,
// timestamps).
fn render(change: &VcsChange) -> String {
    let mut out = String::new();
    out.push_str(&format!("description: {}\n", change.description));
    for file in &change.files {
        let kind = match file.kind {
            VcsFileKind::Added => "A",
            VcsFileKind::Deleted => "D",
            VcsFileKind::Modified => "M",
        };
        out.push_str(&format!("{kind} {}", file.relative_path.display()));
        if file.binary {
            out.push_str(" (binary)");
        }
        out.push('\n');
        for hunk in &file.hunks {
            out.push_str(&format!(
                "  @@ -{},{} +{},{} @@\n",
                hunk.old_lines.start,
                hunk.old_lines.len(),
                hunk.new_lines.start,
                hunk.new_lines.len()
            ));
            for line in &hunk.lines {
                let marker = match line.kind {
                    VcsLineKind::Context => ' ',
                    VcsLineKind::Removed => '-',
                    VcsLineKind::Added => '+',
                };
                out.push_str(&format!("  {marker}{}\n", line.text));
            }
        }
    }
    out
}

fn render_status(poll: &focus::vcs::Poll, root: &Path, path: &str) -> String {
    match poll.statuses.get(&root.join(path)) {
        None => format!("{path}: unchanged\n"),
        Some(status) => {
            let mut out = format!("{path}:\n");
            for range in &status.ranges {
                out.push_str(&format!(
                    "  {}..{} {:?}\n",
                    range.lines.start, range.lines.end, range.kind
                ));
            }
            out
        }
    }
}

// One poll of a repo, straight off the jj side. `Vcs` runs this on its
// worker thread; the tests below run it here, so they see a result rather
// than whatever the worker has got to so far.
fn poll(root: &Path) -> focus::vcs::Poll {
    Poller::new().poll(root)
}

// The change, or the error the poll produced.
fn change(root: &Path) -> VcsChange {
    poll(root).change.unwrap()
}

#[test]
fn added_modified_and_deleted_files() {
    let root = repo_with_base(
        "changes",
        &[
            ("base.txt", "one\ntwo\nthree\nfour\nfive\nsix\nseven\n"),
            ("gone.txt", "bye\n"),
            ("untouched.txt", "still here\n"),
        ],
    );
    std::fs::write(
        root.join("base.txt"),
        "one\ntwo\nCHANGED\nfour\nfive\nsix\nseven\n",
    )
    .unwrap();
    std::fs::remove_file(root.join("gone.txt")).unwrap();
    std::fs::write(root.join("new.txt"), "hello\n").unwrap();

    let change = change(&root);

    assert_eq!(change.root, root);
    assert_eq!(
        render(&change),
        "\
description: the change under test
M base.txt
  @@ -0,6 +0,6 @@
   one
   two
  -three
  +CHANGED
   four
   five
   six
D gone.txt
  @@ -0,1 +0,0 @@
  -bye
A new.txt
  @@ -0,0 +0,1 @@
  +hello
"
    );
}

#[test]
fn file_status_marks_working_copy_lines() {
    let root = repo_with_base(
        "status",
        &[("file.txt", "one\ntwo\nthree\nfour\nfive\nsix\n")],
    );
    // Modify line 2, delete lines 4 and 5, add two lines at the end.
    std::fs::write(
        root.join("file.txt"),
        "one\nTWO\nthree\nsix\nseven\neight\n",
    )
    .unwrap();
    std::fs::write(root.join("added.txt"), "brand new\n").unwrap();

    // One poll fills both the change and the statuses.
    let poll = poll(&root);

    assert_eq!(
        render_status(&poll, &root, "file.txt"),
        "\
file.txt:
  1..2 Modified
  3..3 Deleted
  4..6 Added
"
    );
    assert_eq!(
        render_status(&poll, &root, "added.txt"),
        "\
added.txt:
  0..1 Added
"
    );
}

#[test]
fn two_changes_in_one_hunk_are_two_ranges() {
    // Two changed lines four apart: one hunk at three lines of context, but
    // the gutter has to mark them separately.
    let root = repo_with_base("two_changes", &[("file.txt", "a\nb\nc\nd\ne\nf\ng\nh\n")]);
    std::fs::write(root.join("file.txt"), "a\nB\nc\nd\ne\nF\ng\nh\n").unwrap();

    let poll = poll(&root);

    assert_eq!(
        render(poll.change.as_ref().unwrap()),
        "\
description: the change under test
M file.txt
  @@ -0,8 +0,8 @@
   a
  -b
  +B
   c
   d
   e
  -f
  +F
   g
   h
"
    );
    assert_eq!(
        render_status(&poll, &root, "file.txt"),
        "\
file.txt:
  1..2 Modified
  5..6 Modified
"
    );
}

#[test]
fn unchanged_file_has_no_status() {
    let root = repo_with_base("unchanged", &[("file.txt", "one\n")]);
    std::fs::write(root.join("other.txt"), "new\n").unwrap();

    assert_eq!(
        render_status(&poll(&root), &root, "file.txt"),
        "file.txt: unchanged\n"
    );
}

#[test]
fn ignored_files_are_not_part_of_the_change() {
    let root = repo_with_base("ignored", &[(".gitignore", "ignored/\n")]);
    std::fs::create_dir_all(root.join("ignored")).unwrap();
    std::fs::write(root.join("ignored/junk.txt"), "junk\n").unwrap();
    std::fs::write(root.join("tracked.txt"), "tracked\n").unwrap();

    assert_eq!(
        render(&change(&root)),
        "\
description: the change under test
A tracked.txt
  @@ -0,0 +0,1 @@
  +tracked
"
    );
}

#[test]
fn binary_files_have_no_hunks() {
    let root = repo_with_base("binary", &[]);
    std::fs::write(root.join("blob.bin"), [0u8, 1, 2, 0, 3]).unwrap();

    let poll = poll(&root);

    assert_eq!(
        render(poll.change.as_ref().unwrap()),
        "\
description: the change under test
A blob.bin (binary)
"
    );
    assert_eq!(
        render_status(&poll, &root, "blob.bin"),
        "blob.bin: unchanged\n"
    );
}

#[test]
fn a_directory_that_is_not_a_repo_is_an_error() {
    let root = dir("norepo");
    let poll = poll(&root);
    let error = poll.change.unwrap_err();
    assert!(error.contains("not a jj repo"), "{error}");
    assert!(poll.statuses.is_empty());
}

#[test]
fn polling_does_not_write_operations() {
    let root = repo_with_base("readonly", &[("file.txt", "one\n")]);
    std::fs::write(root.join("file.txt"), "two\n").unwrap();
    let ops = || {
        let mut entries: Vec<PathBuf> =
            std::fs::read_dir(root.join(".jj/repo/op_store/operations"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
        entries.sort();
        entries
    };

    let before = ops();
    let mut poller = Poller::new();
    for _ in 0..5 {
        poller.poll(&root).change.unwrap();
    }

    assert_eq!(before, ops());
}

// Revisions other than the working copy.

// The revisions of a repo built by `repo_with_base`: the change under
// test, and the base commit under it.
fn revisions(root: &Path) -> (VcsRevision, VcsRevision) {
    let revisions = Poller::new().revisions(root).unwrap();
    assert_eq!(revisions.len(), 2, "{revisions:?}");
    let working_copy = revisions[0].clone();
    let base = revisions[1].clone();
    assert!(working_copy.is_working_copy);
    assert!(!base.is_working_copy);
    (working_copy, base)
}

#[test]
fn revisions_are_listed_newest_first() {
    let root = repo_with_base("revisions", &[("file.txt", "base\n")]);

    let (working_copy, base) = revisions(&root);

    assert_eq!(working_copy.description, "the change under test");
    assert_eq!(base.description, "base");
    // A change id is the full reverse hex, which is what asks for it.
    assert_eq!(working_copy.change_id.len(), 32);
    assert_ne!(working_copy.change_id, base.change_id);
}

#[test]
fn another_revision_shows_its_own_change() {
    let root = repo_with_base("other_revision", &[("file.txt", "base\n")]);
    // An edit in the working copy, which the base revision knows nothing
    // about: its change is the files it added.
    std::fs::write(root.join("file.txt"), "changed\n").unwrap();
    let (_, base) = revisions(&root);

    let change = Poller::new()
        .change(&root, base.change_id.as_bstr())
        .unwrap();

    assert_eq!(
        render(&change),
        "\
description: base
A file.txt
  @@ -0,0 +0,1 @@
  +base
"
    );
}

#[test]
fn checking_out_a_revision_records_the_working_copy_first() {
    let root = repo_with_base("checkout", &[("file.txt", "base\n")]);
    std::fs::write(root.join("file.txt"), "working copy\n").unwrap();
    let (working_copy, base) = revisions(&root);

    let mut poller = Poller::new();
    poller
        .checkout(&root, &VcsRevisionId::Change(base.change_id.clone()))
        .unwrap();

    // The base revision's files are the ones on disk now.
    assert_eq!(
        std::fs::read_to_string(root.join("file.txt")).unwrap(),
        "base\n"
    );
    // The edit that was in the working copy was recorded before the
    // checkout wrote over it, so it is still in the revision it was made
    // in. Polling never records, so this is the one thing that does.
    let change = poller
        .change(&root, working_copy.change_id.as_bstr())
        .unwrap();
    assert_eq!(
        render(&change),
        "\
description: the change under test
M file.txt
  @@ -0,1 +0,1 @@
  -base
  +working copy
"
    );
    // ... and `@` is the revision that was checked out.
    let revisions = poller.revisions(&root).unwrap();
    let checked_out = revisions
        .iter()
        .find(|revision| revision.is_working_copy)
        .unwrap();
    assert_eq!(checked_out.change_id, base.change_id);
}

#[test]
fn checking_out_the_revision_already_checked_out_does_nothing() {
    let root = repo_with_base("checkout_noop", &[("file.txt", "base\n")]);
    let (working_copy, _) = revisions(&root);
    let ops = || {
        std::fs::read_dir(root.join(".jj/repo/op_store/operations"))
            .unwrap()
            .count()
    };

    let before = ops();
    Poller::new()
        .checkout(&root, &VcsRevisionId::Change(working_copy.change_id))
        .unwrap();

    // Recording an unchanged working copy writes no operation, and there
    // is nothing to check out.
    assert_eq!(before, ops());
}

#[test]
fn checking_out_the_working_copy_is_nothing_to_do() {
    let root = repo_with_base("checkout_wc", &[("file.txt", "base\n")]);
    Poller::new()
        .checkout(&root, &VcsRevisionId::WorkingCopy)
        .unwrap();
}

// The worker thread. Everything above drives `Poller` directly; these two
// go through the front end the editor uses.

// Wait for `f` to produce a value, as the editor does by asking again on
// the next frame. Fails rather than hanging if the worker never answers.
fn eventually<T>(mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(value) = f() {
            return value;
        }
        assert!(Instant::now() < deadline, "the vcs thread produced nothing");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn the_worker_thread_answers_in_the_background() {
    let root = repo_with_base("threaded", &[("file.txt", "one\ntwo\n")]);
    std::fs::write(root.join("file.txt"), "one\nTWO\n").unwrap();

    let vcs = Vcs::new();
    // Nothing has been polled yet, so the first ask says so rather than
    // blocking.
    let error = vcs
        .change(&root, &VcsRevisionId::WorkingCopy)
        .unwrap_err()
        .to_string();
    assert!(error.contains("reading"), "{error}");
    assert_eq!(vcs.file_status(&root, &root.join("file.txt")), None);

    let change = eventually(|| vcs.change(&root, &VcsRevisionId::WorkingCopy).ok());
    assert_eq!(
        render(&change),
        "\
description: the change under test
M file.txt
  @@ -0,2 +0,2 @@
   one
  -two
  +TWO
"
    );
    let status = eventually(|| vcs.file_status(&root, &root.join("file.txt")));
    assert_eq!(status.ranges.len(), 1);
    assert_eq!(status.ranges[0].lines, 1..2);
}

#[test]
fn the_worker_thread_keeps_up_with_the_working_copy() {
    let root = repo_with_base("threaded_updates", &[("file.txt", "one\n")]);

    let vcs = Vcs::new();
    eventually(|| vcs.change(&root, &VcsRevisionId::WorkingCopy).ok());

    for line in ["two", "three"] {
        std::fs::write(root.join("file.txt"), format!("{line}\n")).unwrap();
        let text = eventually(|| {
            let change = vcs.change(&root, &VcsRevisionId::WorkingCopy).ok()?;
            let hunk = change.files.first()?.hunks.first()?;
            let added = hunk.lines.iter().find(|l| l.text == line.as_bytes())?;
            Some(added.text.to_string())
        });
        assert_eq!(text, line);
    }
}

// Timing, not assertions. See the comment at the top of the file.

fn time_polls(root: &Path, polls: usize) -> (Duration, Vec<Duration>) {
    let mut poller = Poller::new();
    let started = Instant::now();
    poller.poll(root).change.unwrap();
    let first = started.elapsed();
    let mut rest = Vec::new();
    for _ in 0..polls {
        let started = Instant::now();
        poller.poll(root).change.unwrap();
        rest.push(started.elapsed());
    }
    (first, rest)
}

// What the UI thread pays: one ask for a result the worker has already
// produced, which is what `vcs_change` costs per frame.
fn time_asks(root: &Path, asks: usize) -> Vec<Duration> {
    let vcs = Vcs::new();
    let change = eventually(|| vcs.change(root, &VcsRevisionId::WorkingCopy).ok());
    let bytes: usize = change
        .files
        .iter()
        .flat_map(|file| &file.hunks)
        .flat_map(|hunk| &hunk.lines)
        .map(|line| line.text.len())
        .sum();
    println!("  (the change it copies holds {bytes} bytes of diff text)");
    let mut times = Vec::new();
    for _ in 0..asks {
        let started = Instant::now();
        vcs.change(root, &VcsRevisionId::WorkingCopy).unwrap();
        times.push(started.elapsed());
    }
    times
}

fn report(name: &str, first: Duration, rest: &[Duration]) {
    let total: Duration = rest.iter().sum();
    let max = rest.iter().max().copied().unwrap_or_default();
    println!(
        "{name}: first poll (loads the repo) {:?}, then mean {:?}, max {:?} over {} polls",
        first,
        total / rest.len() as u32,
        max,
        rest.len()
    );
}

#[test]
#[ignore]
fn timing_on_this_repo() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let (first, rest) = time_polls(&root, 20);
    report("focus poll", first, &rest);
    let asks = time_asks(&root, 1000);
    let total: Duration = asks.iter().sum();
    println!(
        "focus ask (what a frame pays): mean {:?}, max {:?} over {} asks",
        total / asks.len() as u32,
        asks.iter().max().unwrap(),
        asks.len()
    );
}

#[test]
#[ignore]
fn timing_on_a_big_repo() {
    // 10k files over 100 directories, one of them changed.
    let files: Vec<(String, String)> = (0..10_000)
        .map(|i| {
            (
                format!("dir{}/file{i}.txt", i % 100),
                format!("contents of file {i}\n").repeat(20),
            )
        })
        .collect();
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, contents)| (path.as_str(), contents.as_str()))
        .collect();
    let root = repo_with_base("big", &files);
    std::fs::write(root.join("dir0/file0.txt"), "changed\n").unwrap();

    let (first, rest) = time_polls(&root, 20);
    report("10k files poll", first, &rest);
}
