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

use bstr::{BStr, BString, ByteSlice};
use futures::StreamExt as _;
use jj_lib::backend::{ChangeId, Signature};
use jj_lib::commit::Commit;
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
use jj_lib::hex_util::{decode_reverse_hex, encode_reverse_hex};
use jj_lib::matchers::{EverythingMatcher, NothingMatcher};
use jj_lib::merge::Diff;
use jj_lib::merged_tree::{MergedTree, TreeDiffEntry};
use jj_lib::object_id::ObjectId as _;
use jj_lib::ref_name::WorkspaceName;
use jj_lib::repo::{ReadonlyRepo, Repo as _};
use jj_lib::revset::{ResolvedRevsetExpression, RevsetExpression};
use jj_lib::settings::UserSettings;
use jj_lib::store::Store;
use jj_lib::working_copy::SnapshotOptions;
use jj_lib::workspace::Workspace;
use pollster::block_on;

use focus_core::app::{
    ID_PREFIX_LEN, VcsChange, VcsChangeKind, VcsFile, VcsFileKind, VcsFileStatus, VcsHunk, VcsLine,
    VcsLineKind, VcsLineRange, VcsRevision, VcsRevisionId,
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

// The most revisions the picker is given. Enough to pick from; a repo
// with more history than this is not worth reading in full every poll.
const REVISION_LIMIT: usize = 200;

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
    requests: HashMap<(PathBuf, Request), RequestState>,
    // The checkout each repo was last asked for. One at a time: a
    // checkout moves the whole working copy, so a second one replaces
    // the first rather than queueing behind it.
    checkouts: HashMap<PathBuf, Checkout>,
    exit: bool,
}

/// Something the app wants read, repeatedly, from one repo.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Request {
    /// The working copy: its change, and the per-file line statuses.
    WorkingCopy,
    /// The revisions the picker lists.
    Revisions,
    /// One revision's change, by full change id.
    Change(BString),
}

/// What a request last produced.
enum Answer {
    WorkingCopy(Poll),
    Revisions(Result<Vec<VcsRevision>, String>),
    Change(Result<VcsChange, String>),
}

#[derive(Default)]
struct RequestState {
    // When the app last asked for this.
    wanted: Option<Instant>,
    // When the worker last answered it, and what it found. The answer is
    // shared rather than copied: the app clones out of it without
    // holding the lock.
    polled: Option<Instant>,
    took: Duration,
    answer: Option<Arc<Answer>>,
}

struct Checkout {
    revision: VcsRevisionId,
    // None while it is still running.
    result: Option<Result<(), String>>,
}

/// What one poll of a repo's working copy found.
pub struct Poll {
    pub change: Result<VcsChange, String>,
    /// Keyed by absolute path. Only files with changed lines are in here.
    pub statuses: HashMap<PathBuf, VcsFileStatus>,
}

impl Vcs {
    pub fn new() -> Vcs {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                requests: HashMap::new(),
                checkouts: HashMap::new(),
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

    /// A revision's change, as `jj show <revision>` would show it - as of
    /// the worker's last read of it.
    pub fn change(&self, root: &Path, revision: &VcsRevisionId) -> std::io::Result<VcsChange> {
        let request = match revision {
            VcsRevisionId::WorkingCopy => Request::WorkingCopy,
            VcsRevisionId::Change(change_id) => Request::Change(change_id.clone()),
        };
        match self.answer(root, request).as_deref() {
            Some(Answer::WorkingCopy(poll)) => poll.change.clone().map_err(std::io::Error::other),
            Some(Answer::Change(change)) => change.clone().map_err(std::io::Error::other),
            Some(Answer::Revisions(_)) => unreachable!(),
            None => Err(std::io::Error::other(format!(
                "reading {} ...",
                revision.name()
            ))),
        }
    }

    /// Line status of one working-copy file, against the same base.
    pub fn file_status(&self, root: &Path, path: &Path) -> Option<VcsFileStatus> {
        match self.answer(root, Request::WorkingCopy).as_deref() {
            Some(Answer::WorkingCopy(poll)) => poll.statuses.get(path).cloned(),
            _ => None,
        }
    }

    /// The repo's revisions, newest first.
    pub fn revisions(&self, root: &Path) -> std::io::Result<Vec<VcsRevision>> {
        match self.answer(root, Request::Revisions).as_deref() {
            Some(Answer::Revisions(revisions)) => revisions.clone().map_err(std::io::Error::other),
            Some(_) => unreachable!(),
            None => Err(std::io::Error::other(format!(
                "reading {} ...",
                root.display()
            ))),
        }
    }

    /// Ask for `revision` to be checked out, and report how it went. None
    /// while it is still running, so the caller asks again next frame.
    pub fn checkout(&self, root: &Path, revision: &VcsRevisionId) -> Option<std::io::Result<()>> {
        let mut state = self.shared.state.lock().unwrap();
        let checkout = state
            .checkouts
            .entry(root.to_path_buf())
            .or_insert_with(|| Checkout {
                revision: revision.clone(),
                result: None,
            });
        // A request for a different revision than the one last checked
        // out here starts again: the answer to "is it checked out yet" is
        // about this revision, not the last one.
        if &checkout.revision != revision {
            checkout.revision = revision.clone();
            checkout.result = None;
        }
        let result = checkout.result.clone();
        drop(state);
        if result.is_none() {
            self.shared.wake.notify_all();
        }
        result.map(|result| result.map_err(std::io::Error::other))
    }

    /// The latest answer to a request, and a note to the worker that the
    /// app is still interested. None until the first answer lands.
    fn answer(&self, root: &Path, request: Request) -> Option<Arc<Answer>> {
        let mut state = self.shared.state.lock().unwrap();
        let entry = state
            .requests
            .entry((root.to_path_buf(), request))
            .or_default();
        let first = entry.wanted.is_none();
        entry.wanted = Some(Instant::now());
        let answer = entry.answer.clone();
        drop(state);
        // The worker may be waiting until some other request is due, so
        // wake it for one it has not seen before.
        if first {
            self.shared.wake.notify_all();
        }
        answer
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
    // Check this revision out. Checkouts go first: they are a user
    // waiting on a keypress, not a background read.
    Checkout(PathBuf, VcsRevisionId),
    // Answer this request now.
    Poll(PathBuf, Request),
    // Nothing is due yet; the soonest is this far off.
    Wait(Duration),
    // Nothing is wanted at all.
    Idle,
}

impl State {
    fn next(&mut self, now: Instant) -> Next {
        // Forget what nobody has asked about recently. The worker drops
        // the matching workspaces to match.
        self.requests.retain(|_, request| {
            request
                .wanted
                .is_some_and(|wanted| now.duration_since(wanted) < IDLE_TIMEOUT)
        });

        // A finished checkout is remembered only while something is
        // still reading the repo, so that asking for the same revision
        // again does not get the last checkout's answer.
        let live: Vec<PathBuf> = self.requests.keys().map(|(root, _)| root.clone()).collect();
        self.checkouts
            .retain(|root, checkout| checkout.result.is_none() || live.contains(root));

        for (root, checkout) in &self.checkouts {
            if checkout.result.is_none() {
                return Next::Checkout(root.clone(), checkout.revision.clone());
            }
        }

        let mut soonest: Option<Duration> = None;
        for ((root, request), state) in &self.requests {
            let Some(polled) = state.polled else {
                return Next::Poll(root.clone(), request.clone());
            };
            // A slow read happens less often, so polling can't eat a core
            // no matter how big the change is.
            let gap = POLL_INTERVAL.max(state.took * POLL_BACKOFF);
            let due = polled + gap;
            match due.checked_duration_since(now) {
                None => return Next::Poll(root.clone(), request.clone()),
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
        let next = {
            let mut state = shared.state.lock().unwrap();
            loop {
                if state.exit {
                    return;
                }
                match state.next(Instant::now()) {
                    next @ (Next::Checkout(..) | Next::Poll(..)) => break next,
                    Next::Wait(wait) => {
                        state = shared.wake.wait_timeout(state, wait).unwrap().0;
                    }
                    Next::Idle => state = shared.wake.wait(state).unwrap(),
                }
            }
        };

        match next {
            Next::Checkout(root, revision) => {
                let result = guarded(&root, || poller.checkout(&root, &revision))
                    .unwrap_or_else(|panicked| Err(panicked));
                let mut state = shared.state.lock().unwrap();
                if let Some(checkout) = state.checkouts.get_mut(&root)
                    && checkout.revision == revision
                {
                    checkout.result = Some(result);
                }
            }
            Next::Poll(root, request) => {
                let started = Instant::now();
                let answer = answer(&mut poller, &root, &request);
                let took = started.elapsed();

                let mut state = shared.state.lock().unwrap();
                if let Some(entry) = state.requests.get_mut(&(root.clone(), request)) {
                    entry.polled = Some(Instant::now());
                    entry.took = took;
                    entry.answer = Some(Arc::new(answer));
                }
                // Repos `State::next` has forgotten don't need their
                // workspaces kept open either.
                poller.retain(|root| {
                    state.requests.keys().any(|(wanted, _)| wanted == root)
                        || state.checkouts.contains_key(root)
                });
            }
            Next::Wait(_) | Next::Idle => unreachable!(),
        }
    }
}

fn answer(poller: &mut Poller, root: &Path, request: &Request) -> Answer {
    match request {
        Request::WorkingCopy => Answer::WorkingCopy(
            guarded(root, || poller.poll(root)).unwrap_or_else(|panicked| Poll {
                change: Err(panicked),
                statuses: HashMap::new(),
            }),
        ),
        Request::Revisions => {
            Answer::Revisions(guarded(root, || poller.revisions(root)).unwrap_or_else(Err))
        }
        Request::Change(change_id) => Answer::Change(
            guarded(root, || poller.change(root, change_id.as_bstr())).unwrap_or_else(Err),
        ),
    }
}

// jj-lib panics on some inputs it considers impossible. A panic here would
// leave the thread dead and every later ask waiting for an answer that
// will never come, so it becomes an error the page can show instead.
fn guarded<T>(root: &Path, work: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
        .map_err(|payload| format!("{}: {}", root.display(), panic_message(&*payload)))
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

    /// The repo's revisions, newest first, for the picker.
    pub fn revisions(&mut self, root: &Path) -> Result<Vec<VcsRevision>, String> {
        self.loaded(root)?.revisions()
    }

    /// One revision's change: the commit against its parent. No snapshot,
    /// because these files are not the ones on disk.
    pub fn change(&mut self, root: &Path, change_id: &BStr) -> Result<VcsChange, String> {
        self.loaded(root)?.change(root, change_id)
    }

    /// Check a revision out, so that its files are the ones on disk. The
    /// one thing in here that writes to the repo.
    pub fn checkout(&mut self, root: &Path, revision: &VcsRevisionId) -> Result<(), String> {
        let VcsRevisionId::Change(change_id) = revision else {
            // The working copy is already checked out, by definition.
            return Ok(());
        };
        self.loaded(root)?.checkout(change_id.as_bstr())
    }

    fn loaded(&mut self, root: &Path) -> Result<&mut Loaded, String> {
        self.loaded
            .entry(root.to_path_buf())
            .or_insert_with(|| Loaded::load(root))
            .as_mut()
            .ok_or_else(|| format!("{}: not a jj repo", root.display()))
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
        Ok((change_of(root, &wc_commit, files), statuses))
    }

    fn revisions(&mut self) -> Result<Vec<VcsRevision>, String> {
        let repo = block_on(self.workspace.repo_loader().load_at_head())
            .map_err(|error| error.to_string())?;
        let name = self.workspace.workspace_name().to_owned();
        let wc_id = repo.view().get_wc_commit_id(&name).cloned();
        let root_id = repo.store().root_commit_id().clone();

        // Every visible revision, newest first, which is the order the
        // revset streams them in. jj's own log picks a cleverer set; a
        // picker with a search field does not need one.
        let expression: Arc<ResolvedRevsetExpression> = RevsetExpression::all();
        let revset = expression
            .evaluate(repo.as_ref())
            .map_err(|error| error.to_string())?;

        block_on(async {
            let mut revisions = Vec::new();
            let mut stream = revset.stream();
            while let Some(commit_id) = stream.next().await {
                let commit_id = commit_id.map_err(|error| error.to_string())?;
                // The root commit is jj's own, not a revision anyone is
                // working on.
                if commit_id == root_id {
                    continue;
                }
                let commit = repo
                    .store()
                    .get_commit(&commit_id)
                    .map_err(|error| error.to_string())?;
                revisions.push(VcsRevision {
                    change_id: encode_reverse_hex(commit.change_id().as_bytes()).into(),
                    commit_id: commit.id().hex().into(),
                    author: author_text(commit.author()),
                    description: commit.description().lines().next().unwrap_or("").into(),
                    is_working_copy: Some(&commit_id) == wc_id.as_ref(),
                });
                if revisions.len() >= REVISION_LIMIT {
                    break;
                }
            }
            Ok(revisions)
        })
    }

    fn change(&mut self, root: &Path, change_id: &BStr) -> Result<VcsChange, String> {
        let repo = block_on(self.workspace.repo_loader().load_at_head())
            .map_err(|error| error.to_string())?;
        let commit = resolve(&repo, change_id)?;
        let parent_tree =
            block_on(commit.parent_tree(repo.as_ref())).map_err(|error| error.to_string())?;
        let (files, _statuses) = read_diff(repo.store(), root, &parent_tree, &commit.tree())?;
        Ok(change_of(root, &commit, files))
    }

    // Put a revision's files in the working copy, by making a new empty
    // change on top of it - `jj new <revision>`, not `jj edit`. Landing
    // on the revision itself would mean that typing in the editor amends
    // it, which is not what looking at a line of it should do, and jj
    // refuses it outright for an immutable revision.
    fn checkout(&mut self, change_id: &BStr) -> Result<(), String> {
        // Recording first is what makes this safe: our polling snapshots
        // are deliberately not recorded, so the changes made since the
        // last real jj command exist only as files on disk, and checking
        // out would write over them.
        let repo = self.record_snapshot()?;
        let commit = resolve(&repo, change_id)?;
        let name = self.workspace.workspace_name().to_owned();
        let old_commit = wc_commit(&repo, &name)?;
        // Already looking at it: `@` is the revision itself, or the
        // empty change a previous jump into it left on top.
        if old_commit.id() == commit.id() || is_empty_child_of(&old_commit, &commit) {
            return Ok(());
        }

        let mut tx = repo.start_transaction();
        let mut_repo = tx.repo_mut();
        let new_commit = block_on(
            mut_repo
                .new_commit(vec![commit.id().clone()], commit.tree())
                .write(),
        )
        .map_err(|error| error.to_string())?;
        // Moving `@` abandons the change being left if it is empty and
        // undescribed - jj does that on every command, and only for a
        // change nothing else points at and nothing sits on top of.
        block_on(mut_repo.edit(name, &new_commit)).map_err(|error| error.to_string())?;
        block_on(mut_repo.rebase_descendants()).map_err(|error| error.to_string())?;
        let repo = block_on(tx.commit("new empty commit")).map_err(|error| error.to_string())?;

        block_on(self.workspace.check_out(
            repo.op_id().clone(),
            Some(&old_commit.tree()),
            &new_commit,
        ))
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    // Snapshot the working copy and write it down: a new tree for `@`, an
    // operation recording it, and an updated working-copy state. This is
    // what every jj command does, and the only time we do it.
    fn record_snapshot(&mut self) -> Result<Arc<ReadonlyRepo>, String> {
        let repo = block_on(self.workspace.repo_loader().load_at_head())
            .map_err(|error| error.to_string())?;
        let name = self.workspace.workspace_name().to_owned();
        let old_commit = wc_commit(&repo, &name)?;

        let mut locked_ws = block_on(self.workspace.start_working_copy_mutation())
            .map_err(|error| error.to_string())?;
        let (tree, _stats) = block_on(locked_ws.locked_wc().snapshot(&snapshot_options()))
            .map_err(|error| error.to_string())?;

        let repo = if tree.tree_ids_and_labels() == old_commit.tree().tree_ids_and_labels() {
            repo
        } else {
            let mut tx = repo.start_transaction();
            let mut_repo = tx.repo_mut();
            let new_commit = block_on(mut_repo.rewrite_commit(&old_commit).set_tree(tree).write())
                .map_err(|error| error.to_string())?;
            mut_repo
                .set_wc_commit(name.clone(), new_commit.id().clone())
                .map_err(|error| error.to_string())?;
            block_on(mut_repo.rebase_descendants()).map_err(|error| error.to_string())?;
            block_on(tx.commit("snapshot working copy")).map_err(|error| error.to_string())?
        };
        block_on(locked_ws.finish(repo.op_id().clone())).map_err(|error| error.to_string())?;
        Ok(repo)
    }

    // Snapshot the working copy into a tree, and drop the lock without
    // calling `finish`: we read the working copy, we never record it.
    fn snapshot(&mut self) -> Result<MergedTree, String> {
        let mut locked_ws = block_on(self.workspace.start_working_copy_mutation())
            .map_err(|error| error.to_string())?;
        let (tree, _stats) = block_on(locked_ws.locked_wc().snapshot(&snapshot_options()))
            .map_err(|error| error.to_string())?;
        Ok(tree)
    }
}

fn snapshot_options<'a>() -> SnapshotOptions<'a> {
    SnapshotOptions {
        // Only the user's global excludes file would go here; the in-tree
        // .gitignores are read by the snapshotter itself.
        base_ignores: GitIgnoreFile::empty(),
        progress: None,
        // jj's default `snapshot.auto-track` is `all()`, so a new file
        // shows up as added without being tracked first.
        start_tracking_matcher: &EverythingMatcher,
        force_tracking_matcher: &NothingMatcher,
        max_new_file_size: MAX_NEW_FILE_SIZE,
    }
}

fn wc_commit(repo: &Arc<ReadonlyRepo>, name: &WorkspaceName) -> Result<Commit, String> {
    let commit_id = repo
        .view()
        .get_wc_commit_id(name)
        .ok_or_else(|| "no working-copy commit".to_string())?;
    repo.store()
        .get_commit(commit_id)
        .map_err(|error| error.to_string())
}

// The commit a full change id names. A change can have several commits
// after a rewrite; the first visible one is the current version of it.
fn resolve(repo: &Arc<ReadonlyRepo>, change_id: &BStr) -> Result<Commit, String> {
    let bytes =
        decode_reverse_hex(change_id).ok_or_else(|| format!("{change_id}: not a change id"))?;
    let targets = block_on(repo.resolve_change_id(&ChangeId::new(bytes)))
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("{change_id}: no such revision"))?;
    let (_, commit_id) = targets
        .visible_with_offsets()
        .next()
        .ok_or_else(|| format!("{change_id}: revision is hidden"))?;
    repo.store()
        .get_commit(commit_id)
        .map_err(|error| error.to_string())
}

// `@` is an empty change sitting on top of `commit`: the state a previous
// jump into that revision left behind.
fn is_empty_child_of(wc_commit: &Commit, commit: &Commit) -> bool {
    wc_commit.parent_ids().len() == 1
        && &wc_commit.parent_ids()[0] == commit.id()
        && wc_commit.tree().tree_ids_and_labels() == commit.tree().tree_ids_and_labels()
}

fn change_of(root: &Path, commit: &Commit, files: Vec<VcsFile>) -> VcsChange {
    VcsChange {
        root: root.to_path_buf(),
        change_id: encode_reverse_hex(commit.change_id().as_bytes())[..ID_PREFIX_LEN].into(),
        commit_id: commit.id().hex()[..ID_PREFIX_LEN].into(),
        author: author_text(commit.author()),
        description: commit.description().into(),
        files,
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
