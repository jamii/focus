// Searching the repo, off the frame.
//
// A search walks every file in the repo and reads it, which is nowhere
// near a frame's work on anything but a small tree - and the page asks
// for one on every keystroke. So it happens on a worker thread, the same
// way the jj reads in `vcs` do: the app asks for a pattern, gets None
// while the answer is still being worked out, and asks again next frame.
//
// Like `vcs`, what is wanted is a map rather than a single slot. Every
// open search page asks for its own pattern every frame, so two of them
// sharing one slot would overwrite each other, and which search ran
// would come down to which page asked last. They are keyed and taken in
// turn instead, and an answer lives for exactly as long as something is
// still asking for it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bstr::{BStr, BString};

use focus_core::app::RepoSearch;

// A pattern nobody has asked about for this long is forgotten, along
// with whatever it found. A page asks for its pattern every frame, so
// this is sixty frames of grace for one that is still on screen - and it
// is what keeps a fast typist's abandoned patterns, and the matches they
// found, from piling up.
const IDLE_TIMEOUT: Duration = Duration::from_secs(1);

/// One search, and what it is a search for.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Query {
    dir: PathBuf,
    pattern: BString,
    match_limit: usize,
    line_limit: usize,
}

struct RequestState {
    // When the app last asked for this. Expiry is off this.
    wanted: Instant,
    // When it was first asked for, so that the worker can take searches
    // in the order they were wanted. Two pages both waiting get served
    // one after the other rather than by whoever asked most recently.
    first_wanted: Instant,
    // None until the worker has run it.
    answer: Option<Arc<std::io::Result<RepoSearch>>>,
}

/// The app's side: asking is a hash lookup, and answers are whatever the
/// worker has finished.
pub struct Search {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    // Woken when a pattern is asked for, and when the worker should exit.
    wake: Condvar,
}

#[derive(Default)]
struct State {
    requests: HashMap<Query, RequestState>,
    exit: bool,
}

impl Search {
    pub fn new() -> Search {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
        });
        {
            let shared = shared.clone();
            thread::Builder::new()
                .name("search".to_string())
                .spawn(move || run(&shared))
                .expect("spawn the search thread");
        }
        Search { shared }
    }

    /// What searching `dir` for `pattern` found, or None while it is
    /// still being searched for - so the caller asks again next frame.
    pub fn search(
        &self,
        dir: &Path,
        pattern: &BStr,
        match_limit: usize,
        line_limit: usize,
    ) -> Option<std::io::Result<RepoSearch>> {
        let query = Query {
            dir: dir.to_path_buf(),
            pattern: pattern.into(),
            match_limit,
            line_limit,
        };
        let now = Instant::now();
        let mut state = self.shared.state.lock().unwrap();
        let mut first = false;
        let request = state.requests.entry(query).or_insert_with(|| {
            first = true;
            RequestState {
                wanted: now,
                first_wanted: now,
                answer: None,
            }
        });
        request.wanted = now;
        let answer = request.answer.clone();
        drop(state);
        // The worker is asleep unless there is something to do, so a
        // pattern it has not seen before has to wake it.
        if first {
            self.shared.wake.notify_all();
        }
        answer.map(|answer| match answer.as_ref() {
            Ok(search) => Ok(search.clone()),
            Err(error) => Err(std::io::Error::other(error.to_string())),
        })
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().exit = true;
        self.shared.wake.notify_all();
        // Not joined, unlike the vcs worker. A search only reads, so
        // there is no lock to release and nothing half-written to
        // finish, and waiting for one to walk a big repo would be an
        // editor that takes seconds to quit. The thread holds its own
        // handle on `shared`, so letting it run out on its own is safe.
    }
}

impl State {
    /// The search to run next: the one wanted longest ago that has no
    /// answer yet.
    fn next(&mut self, now: Instant) -> Option<Query> {
        // Forget what nobody has asked about recently, answers included.
        // A page still showing a pattern's matches is still asking for
        // them every frame, so this only ever drops what nothing is
        // looking at.
        self.requests
            .retain(|_, request| now.duration_since(request.wanted) < IDLE_TIMEOUT);

        self.requests
            .iter()
            .filter(|(_, request)| request.answer.is_none())
            .min_by_key(|(_, request)| request.first_wanted)
            .map(|(query, _)| query.clone())
    }
}

fn run(shared: &Shared) {
    loop {
        let mut state = shared.state.lock().unwrap();
        if state.exit {
            return;
        }
        let Some(query) = state.next(Instant::now()) else {
            // Nothing to search for. Wake on the timeout as well as on a
            // request, so that abandoned patterns are forgotten rather
            // than held until someone happens to ask for something else.
            let (unused, _timeout) = shared.wake.wait_timeout(state, IDLE_TIMEOUT).unwrap();
            drop(unused);
            continue;
        };
        drop(state);

        let answer = Arc::new(crate::chrome::repo_search(
            &query.dir,
            query.pattern.as_ref(),
            query.match_limit,
            query.line_limit,
        ));

        let mut state = shared.state.lock().unwrap();
        // Gone if nobody asked for it while the search was running, in
        // which case the answer is not wanted either.
        if let Some(request) = state.requests.get_mut(&query) {
            request.answer = Some(answer);
        }
    }
}
