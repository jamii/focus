// The jj side of the VCS integration.
//
// focus-core asks for `vcs_change` and `vcs_file_status` every frame, for
// every visible file-backed editor, and those calls are on the UI thread,
// so nothing here may block: a poll takes milliseconds, and it waits on
// jj's working-copy lock, which a `jj` command in a terminal can be
// holding.
//
// So the jj work happens on a worker thread. `Vcs` is the front end: it
// records which repos the app is asking about, hands back the most recent
// result for each, and never waits. `Poller` is the jj side, and is only
// ever touched by that thread - jj's `Workspace` and everything reachable
// from it stays on the one thread, and only plain data crosses over. One
// poll of a repo produces both the whole change (for the diff page) and the
// per-file line status (for the gutters).
//
// We snapshot the working copy, like every jj command does, so that what we
// show matches what `jj show @` would show. Unlike a jj command, we do not
// record the snapshot: no transaction, no operation, no update of the
// working copy's tree state. An editor polling in the background must not
// write to the op log. The cost is that each poll re-reads the files you
// have changed since the last recorded snapshot, because the tree state we
// would have updated is the thing that would have let us skip them.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bstr::{BString, ByteSlice};
use futures::StreamExt as _;
use jj_lib::backend::Signature;
use jj_lib::config::StackedConfig;
use jj_lib::conflict_labels::ConflictLabels;
use jj_lib::conflicts::{ConflictMarkerStyle, ConflictMaterializeOptions, materialize_tree_value};
use jj_lib::default_backend_factories::{
    default_backend_factories, default_working_copy_factories,
};
use jj_lib::diff_presentation::LineCompareMode;
use jj_lib::diff_presentation::unified::{
    DiffLineType, UnifiedDiffHunk, git_diff_part, unified_diff_hunks,
};
use jj_lib::gitignore::GitIgnoreFile;
use jj_lib::hex_util::encode_reverse_hex;
use jj_lib::matchers::{EverythingMatcher, NothingMatcher};
use jj_lib::merge::Diff;
use jj_lib::merged_tree::{MergedTree, TreeDiffEntry};
use jj_lib::object_id::ObjectId as _;
use jj_lib::repo::Repo as _;
use jj_lib::settings::UserSettings;
use jj_lib::store::Store;
use jj_lib::working_copy::SnapshotOptions;
use jj_lib::workspace::Workspace;
use pollster::block_on;

use focus_core::app::{
    VcsChange, VcsChangeKind, VcsFile, VcsFileKind, VcsFileStatus, VcsHunk, VcsLine, VcsLineKind,
    VcsLineRange,
};

// The shortest gap between two polls of one repo. The app asks every
// frame; a snapshot walks the whole working copy, which is nowhere near a
// frame's work.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

// A repo whose last poll took `took` is not polled again for `took *
// POLL_BACKOFF`, so a repo with a big enough change to be slow costs at
// most a fraction of one core rather than a whole one.
const POLL_BACKOFF: u32 = 4;

// A repo nobody has asked about for this long is forgotten, along with its
// loaded workspace - a window closed on a repo stops costing anything.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5);

// jj's default for `snapshot.max-new-file-size`. We read jj's built-in
// config rather than the user's, so their override does not reach us.
const MAX_NEW_FILE_SIZE: u64 = 1024 * 1024;

// Lines of context around each hunk of the diff page, as `jj show` uses.
const CONTEXT_LINES: usize = 3;

// How many hex digits of a change or commit id to show, as jj does.
const ID_PREFIX_LEN: usize = 12;

/// The app's side: asking about a repo is a hash lookup and a timestamp,
/// and answers are whatever the worker thread last produced.
pub struct Vcs {
    shared: Arc<Shared>,
    worker: Option<thread::JoinHandle<()>>,
}

struct Shared {
    state: Mutex<State>,
    // Woken when a repo is asked about, and when the worker should exit.
    wake: Condvar,
}

struct State {
    // Keyed by workspace root.
    repos: HashMap<PathBuf, RepoState>,
    exit: bool,
}

#[derive(Default)]
struct RepoState {
    // When the app last asked about this repo.
    wanted: Option<Instant>,
    // When the worker last finished polling it, and what it found. The
    // result is shared rather than copied: the app clones out of it
    // without holding the lock.
    polled: Option<Instant>,
    took: Duration,
    result: Option<Arc<Poll>>,
}

/// What one poll of a repo found.
pub struct Poll {
    pub change: Result<VcsChange, String>,
    /// Keyed by absolute path. Only files with changed lines are in here.
    pub statuses: HashMap<PathBuf, VcsFileStatus>,
}

impl Vcs {
    pub fn new() -> Vcs {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                repos: HashMap::new(),
                exit: false,
            }),
            wake: Condvar::new(),
        });
        let worker = {
            let shared = shared.clone();
            thread::Builder::new()
                .name("vcs".to_string())
                .spawn(move || run(&shared))
                .expect("spawn the vcs thread")
        };
        Vcs {
            shared,
            worker: Some(worker),
        }
    }

    /// The working-copy revision's change, as `jj show @` would show it -
    /// as of the worker's last poll of this repo.
    pub fn change(&self, root: &Path) -> std::io::Result<VcsChange> {
        match self.result(root) {
            Some(poll) => poll.change.clone().map_err(std::io::Error::other),
            None => Err(std::io::Error::other(format!(
                "reading {} ...",
                root.display()
            ))),
        }
    }

    /// Line status of one working-copy file, against the same base.
    pub fn file_status(&self, root: &Path, path: &Path) -> Option<VcsFileStatus> {
        self.result(root)?.statuses.get(path).cloned()
    }

    /// The latest poll of `root`, and a note to the worker that the app is
    /// still interested. None until the first poll of a repo lands.
    fn result(&self, root: &Path) -> Option<Arc<Poll>> {
        let mut state = self.shared.state.lock().unwrap();
        let repo = state.repos.entry(root.to_path_buf()).or_default();
        let first = repo.wanted.is_none();
        repo.wanted = Some(Instant::now());
        let result = repo.result.clone();
        drop(state);
        // The worker may be waiting until some other repo is due, so wake
        // it for a repo it has not seen before.
        if first {
            self.shared.wake.notify_all();
        }
        result
    }
}

impl Drop for Vcs {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().exit = true;
        self.shared.wake.notify_all();
        // A poll in flight holds jj's working-copy lock, so wait for it
        // rather than leaving the lock to be cleaned up by the OS.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

// What the worker should do next.
enum Next {
    // Poll this repo now.
    Poll(PathBuf),
    // Nothing is due yet; the soonest is this far off.
    Wait(Duration),
    // No repo is wanted at all.
    Idle,
}

impl State {
    fn next(&mut self, now: Instant) -> Next {
        // Forget repos nobody has asked about recently. The worker drops
        // their workspaces to match.
        self.repos.retain(|_, repo| {
            repo.wanted
                .is_some_and(|wanted| now.duration_since(wanted) < IDLE_TIMEOUT)
        });
        let mut soonest: Option<Duration> = None;
        for (root, repo) in &self.repos {
            let Some(polled) = repo.polled else {
                return Next::Poll(root.clone());
            };
            // A slow repo is polled less often, so polling can't eat a
            // core no matter how big the change is.
            let gap = POLL_INTERVAL.max(repo.took * POLL_BACKOFF);
            let due = polled + gap;
            match due.checked_duration_since(now) {
                None => return Next::Poll(root.clone()),
                Some(wait) => soonest = Some(soonest.map_or(wait, |s: Duration| s.min(wait))),
            }
        }
        match soonest {
            Some(wait) => Next::Wait(wait),
            None => Next::Idle,
        }
    }
}

// The worker thread. Every jj object lives here.
fn run(shared: &Shared) {
    let mut poller = Poller::new();
    loop {
        let root = {
            let mut state = shared.state.lock().unwrap();
            loop {
                if state.exit {
                    return;
                }
                match state.next(Instant::now()) {
                    Next::Poll(root) => break root,
                    Next::Wait(wait) => {
                        state = shared.wake.wait_timeout(state, wait).unwrap().0;
                    }
                    Next::Idle => state = shared.wake.wait(state).unwrap(),
                }
            }
        };

        let started = Instant::now();
        // jj-lib panics on some inputs it considers impossible. A panic
        // here would leave the thread dead and every later ask waiting
        // for a result that will never come, so it becomes an error the
        // diff page can show instead.
        let poll = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| poller.poll(&root)))
            .unwrap_or_else(|payload| Poll {
                change: Err(format!("{}: {}", root.display(), panic_message(&*payload))),
                statuses: HashMap::new(),
            });
        let took = started.elapsed();

        let mut state = shared.state.lock().unwrap();
        if let Some(repo) = state.repos.get_mut(&root) {
            repo.polled = Some(Instant::now());
            repo.took = took;
            repo.result = Some(Arc::new(poll));
        }
        // Repos `State::next` has forgotten don't need their workspaces
        // kept open either.
        poller.retain(|root| state.repos.contains_key(root));
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "panicked".to_string()
}

/// The jj side, synchronous. Lives on the worker thread, and is the only
/// thing in here that tests drive directly.
pub struct Poller {
    // Keyed by workspace root. None means "looked, and it is not a jj
    // workspace" - remembered so we don't retry the load every poll.
    loaded: HashMap<PathBuf, Option<Loaded>>,
}

struct Loaded {
    workspace: Workspace,
}

impl Poller {
    pub fn new() -> Poller {
        Poller {
            loaded: HashMap::new(),
        }
    }

    /// Snapshot the working copy of the repo at `root` and diff it against
    /// its parent.
    pub fn poll(&mut self, root: &Path) -> Poll {
        let loaded = self
            .loaded
            .entry(root.to_path_buf())
            .or_insert_with(|| Loaded::load(root));
        let Some(loaded) = loaded else {
            return Poll {
                change: Err(format!("{}: not a jj repo", root.display())),
                statuses: HashMap::new(),
            };
        };
        match loaded.read(root) {
            Ok((change, statuses)) => Poll {
                change: Ok(change),
                statuses,
            },
            Err(error) => Poll {
                change: Err(error),
                statuses: HashMap::new(),
            },
        }
    }

    fn retain(&mut self, keep: impl Fn(&Path) -> bool) {
        self.loaded.retain(|root, _| keep(root));
    }
}

impl Loaded {
    fn load(root: &Path) -> Option<Loaded> {
        // jj's built-in defaults, not the user's config: nothing we do
        // writes commits or operations, so the only setting that would
        // matter is `snapshot.max-new-file-size`.
        let settings = UserSettings::from_config(StackedConfig::with_defaults()).ok()?;
        let workspace = Workspace::load(
            &settings,
            root,
            &default_backend_factories(),
            &default_working_copy_factories(),
        )
        .ok()?;
        Some(Loaded { workspace })
    }

    // `root` is the path the app asked about, which is what the paths we
    // hand back are built from - the app looks its files up by the names
    // it knows them by, not by whatever spelling jj canonicalized to.
    fn read(
        &mut self,
        root: &Path,
    ) -> Result<(VcsChange, HashMap<PathBuf, VcsFileStatus>), String> {
        let repo = block_on(self.workspace.repo_loader().load_at_head())
            .map_err(|error| error.to_string())?;
        let name = self.workspace.workspace_name().to_owned();
        let wc_id = repo
            .view()
            .get_wc_commit_id(&name)
            .ok_or_else(|| format!("{}: no working-copy commit", root.display()))?
            .clone();
        let wc_commit = repo
            .store()
            .get_commit(&wc_id)
            .map_err(|error| error.to_string())?;

        let new_tree = self.snapshot()?;
        let parent_tree =
            block_on(wc_commit.parent_tree(repo.as_ref())).map_err(|error| error.to_string())?;

        let (files, statuses) = read_diff(repo.store(), root, &parent_tree, &new_tree)?;
        let change = VcsChange {
            root: root.to_path_buf(),
            change_id: encode_reverse_hex(wc_commit.change_id().as_bytes())[..ID_PREFIX_LEN].into(),
            commit_id: wc_commit.id().hex()[..ID_PREFIX_LEN].into(),
            author: author_text(wc_commit.author()),
            description: wc_commit.description().into(),
            files,
        };
        Ok((change, statuses))
    }

    // Snapshot the working copy into a tree, and drop the lock without
    // calling `finish`: we read the working copy, we never record it.
    fn snapshot(&mut self) -> Result<MergedTree, String> {
        let mut locked_ws = block_on(self.workspace.start_working_copy_mutation())
            .map_err(|error| error.to_string())?;
        let options = SnapshotOptions {
            // Only the user's global excludes file would go here; the
            // in-tree .gitignores are read by the snapshotter itself.
            base_ignores: GitIgnoreFile::empty(),
            progress: None,
            // jj's default `snapshot.auto-track` is `all()`, so a new file
            // shows up as added without being tracked first.
            start_tracking_matcher: &EverythingMatcher,
            force_tracking_matcher: &NothingMatcher,
            max_new_file_size: MAX_NEW_FILE_SIZE,
        };
        let (tree, _stats) = block_on(locked_ws.locked_wc().snapshot(&options))
            .map_err(|error| error.to_string())?;
        Ok(tree)
    }
}

fn read_diff(
    store: &Store,
    root: &Path,
    parent_tree: &MergedTree,
    new_tree: &MergedTree,
) -> Result<(Vec<VcsFile>, HashMap<PathBuf, VcsFileStatus>), String> {
    let materialize_options = ConflictMaterializeOptions {
        marker_style: ConflictMarkerStyle::Diff,
        marker_len: None,
        merge: store.merge_options().clone(),
    };
    let labels = ConflictLabels::unlabeled();

    block_on(async {
        let mut stream = parent_tree.diff_stream(new_tree, &EverythingMatcher);
        let mut files = Vec::new();
        let mut statuses = HashMap::new();
        while let Some(TreeDiffEntry { path, values }) = stream.next().await {
            let Diff { before, after } = values.map_err(|error| error.to_string())?;
            let before = materialize_tree_value(store, &path, before, &labels)
                .await
                .map_err(|error| error.to_string())?;
            let after = materialize_tree_value(store, &path, after, &labels)
                .await
                .map_err(|error| error.to_string())?;
            let before = git_diff_part(&path, before, &materialize_options)
                .await
                .map_err(|error| error.to_string())?;
            let after = git_diff_part(&path, after, &materialize_options)
                .await
                .map_err(|error| error.to_string())?;

            // A part with no mode is an absent file: on the left it is an
            // add, on the right a delete.
            let kind = match (before.mode.is_none(), after.mode.is_none()) {
                (true, _) => VcsFileKind::Added,
                (_, true) => VcsFileKind::Deleted,
                _ => VcsFileKind::Modified,
            };
            let binary = before.content.is_binary || after.content.is_binary;
            let relative_path = path
                .to_fs_path(Path::new(""))
                .map_err(|error| error.to_string())?;

            let contents = Diff::new(
                before.content.contents.as_bstr(),
                after.content.contents.as_bstr(),
            );
            let mut hunks = Vec::new();
            let mut ranges = Vec::new();
            if !binary {
                // Line-diffing is the most expensive part of a poll, so the
                // gutter's line ranges are read back out of the same hunks
                // the diff page renders rather than diffed again.
                for hunk in unified_diff_hunks(contents, CONTEXT_LINES, LineCompareMode::Exact) {
                    line_ranges(&hunk, &mut ranges);
                    hunks.push(to_hunk(&hunk));
                }
            }
            if !ranges.is_empty() && kind != VcsFileKind::Deleted {
                statuses.insert(root.join(&relative_path), VcsFileStatus { ranges });
            }

            files.push(VcsFile {
                relative_path,
                kind,
                binary,
                hunks,
            });
        }
        Ok((files, statuses))
    })
}

fn to_hunk(hunk: &UnifiedDiffHunk<'_>) -> VcsHunk {
    VcsHunk {
        old_lines: hunk.left_line_range.clone(),
        new_lines: hunk.right_line_range.clone(),
        lines: hunk
            .lines
            .iter()
            .map(|(kind, tokens)| VcsLine {
                kind: match kind {
                    DiffLineType::Context => VcsLineKind::Context,
                    DiffLineType::Removed => VcsLineKind::Removed,
                    DiffLineType::Added => VcsLineKind::Added,
                },
                text: line_text(tokens),
            })
            .collect(),
    }
}

// The changed regions of one hunk, as ranges of working-copy lines,
// appended to `ranges`. A hunk holds context lines as well as changes, so
// one hunk can hold several regions - each maximal run of added and removed
// lines is one. A run with no added lines has an empty range: the lines it
// describes are gone, so it marks the boundary they were deleted from.
fn line_ranges(hunk: &UnifiedDiffHunk<'_>, ranges: &mut Vec<VcsLineRange>) {
    // The line of the working-copy file the next non-removed line sits on.
    let mut line = hunk.right_line_range.start;
    let mut run: Option<(Range<usize>, bool, bool)> = None;
    for (kind, _) in &hunk.lines {
        match kind {
            DiffLineType::Context => {
                if let Some(run) = run.take() {
                    ranges.push(to_line_range(run));
                }
                line += 1;
            }
            DiffLineType::Added => {
                let (lines, added, _) = run.get_or_insert((line..line, false, false));
                *added = true;
                lines.end = line + 1;
                line += 1;
            }
            DiffLineType::Removed => {
                let (_, _, removed) = run.get_or_insert((line..line, false, false));
                *removed = true;
            }
        }
    }
    if let Some(run) = run {
        ranges.push(to_line_range(run));
    }
}

fn to_line_range((lines, added, removed): (Range<usize>, bool, bool)) -> VcsLineRange {
    let kind = match (added, removed) {
        (true, true) => VcsChangeKind::Modified,
        (true, false) => VcsChangeKind::Added,
        // A run always holds a change, so this is the delete case.
        (false, _) => VcsChangeKind::Deleted,
    };
    VcsLineRange { lines, kind }
}

// The tokens of one diff line, joined, without the trailing newline. The
// tokens are jj's word-level refinement, which we don't render.
fn line_text(tokens: &[(jj_lib::diff_presentation::DiffTokenType, &[u8])]) -> BString {
    let mut text: Vec<u8> = Vec::new();
    for (_, token) in tokens {
        text.extend_from_slice(token);
    }
    while text.last() == Some(&b'\n') || text.last() == Some(&b'\r') {
        text.pop();
    }
    BString::from(text)
}

fn author_text(author: &Signature) -> BString {
    let timestamp = match author.timestamp.to_datetime() {
        Ok(datetime) => datetime.format("%Y-%m-%d %H:%M:%S").to_string(),
        Err(_) => "(unknown time)".to_string(),
    };
    BString::from(format!(
        "{} <{}> ({})",
        author.name, author.email, timestamp
    ))
}
