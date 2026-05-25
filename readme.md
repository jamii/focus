## wishlist

* more text structure 
  * https://matklad.github.io/2023/03/08/an-engine-for-an-editor.html
  * https://github.com/matklad/abont
  * structured forms rather than adhoc ui
  * eg code folding - surprisingly hard to support simply
  * in general want it to be much easier to add ui. not custom per task.
* testability
  * pass IO, support DST
    * think about whether we want to handle IO with async or with polling (eg subprocesses)
      * async is a bitch to debug
      * polling sometimes requires manual state machines
  * render image snapshots from test and produce a html doc with diffs
  * ability to record and replay snapshot tests from the editor
  * fuzz the UI (random inputs) and record crashes as snapshot tests
  * can property test a lot of editing invariants
  * can do non-simulated tests (eg shelling out to other tools) in a bubblewrapped directory
  * useful for vibecoding/debugging parts of the editor
* hot code reloading
  * snapshot all state and reload new code
  * keep parent process alive so we don't kill windows / subprocesses
  * can we reasonably reload from a snapshot on crash?
    * rerunning subprocesses isn't always safe - maybe mark them as killed and have a command to rerun
* a debugger view to render internal state as inspector tree
  * good test of the design of structured text ui
* terminal editor
  * ghostty helpful here?
  * https://matklad.github.io/2025/08/31/vibe-coding-terminal-editor.html
* ability to open nix shells?
* very basic unicode - at least tofu. probably still monospace though
* mass edit from eg project-wide search
  * readonly separators
  * map edits on ranges to edits on underlying buffers
    * this is one more thing that needs to be able to subscribe to edits on a buffer
* undo tree
* lsp
* magit-style menus (https://github.com/magit/transient / https://github.com/positron-solutions/transient-showcase)
* magit (https://matklad.github.io/2026/03/05/jj-lsp-followup.html)
* file tree / dired
* tentative ideas for agent integration
  * point agent at special comments, open magit diff, add review comments, run again
  * conversation tree?

## next

Editor:
* Undo/redo
* Loading/saving
* Open on web

Language:
* Tokenize + highlight
* Comment/uncomment
* Smart indent
* Paren matching
* Structural move/select
* Goto definition
* Completion ui
* Completion provider

## notes

Loading/saving on master (lib/focus/buffer.zig, plus call sites in lib/focus.zig, lib/focus/window.zig, lib/focus/editor.zig):
* `BufferSource` is a tagged union: `.None` (scratch) or `.File { absolute_filename, mtime }`. Buffer also carries `modified_since_last_save` and `deleted_since_last_save` flags
* buffers are cached app-wide by absolute filename: `App.getBufferFromAbsoluteFilename` returns the existing buffer or creates one with `Buffer.initFromAbsoluteFilename`, then calls `refresh()` once to pick up any external changes
* `initFromAbsoluteFilename` picks the `Language` from the filename, sets `source.File` with `mtime = 0`, then calls `load(.Init)`, then resets `undos` and `modified_since_last_save = false` (the initial load is not undoable)
* `tryLoad` reads via `std.fs.cwd().openFile`, into a `frame_allocator` slice sized by `stat().size`. If `options.limit_load_bytes` is set, the read is capped at `limited_load_bytes` (200*500) — used by previews
* `load(kind)`:
  * passes raw bytes through `language.afterLoad` (normalization hook, e.g. line endings)
  * `.Init` → `rawReplace` (no undo entry); `.Refresh` → `replace` (creates an undo entry, but only if bytes differ — `replace` early-exits when equal)
  * updates `source.File.mtime` to the stat'd value
  * on error, replaces the buffer with the formatted error string instead of the file contents (so the buffer shows the error inline)
* `refresh()`: stats the file; if `mtime` differs from the stored one, calls `load(.Refresh)`. If the file is missing (`FileNotFound`), sets both `modified_since_last_save` and `deleted_since_last_save` and keeps the in-memory contents
* `App.frame` calls `refresh()` every frame on the top editor of each window (only visible buffers are polled) before running window frames
* `save(source)` — `source` is `User` or `Auto`:
  * `.User` → `createFile(truncate=true)` (creates the file if missing — re-creates after external delete)
  * `.Auto` → `openFile(write_only)` + `setEndPos(0)` + `seekTo(0)`; on `FileNotFound` it silently bails out and just sets `modified_since_last_save = true` (autosave will not recreate a file the user deleted)
  * bytes go through `language.beforeSave` (formatter hook) before being written; mtime is re-stat'd post-write; `modified_since_last_save` and `deleted_since_last_save` are cleared; then `app.handleAfterSave()` fires
* `Editor.save(source)` wraps `Buffer.save`: it no-ops when `!modified_since_last_save`, and on `.User` it calls `tryFormat()` (runs `language.format` and `replace`s if it returns non-null) before saving
* Ctrl-S → `editor.save(.User)`. Autosave (`editor.save(.Auto)`) fires on:
  * window `focus_lost`
  * `Window.pushView` and `Window.popView` whenever the current top view is an Editor (so leaving an editor for the file opener, project searcher, maker, etc. saves first)
  * `Window.deinitPoppedViews` (every popped Editor saves on destruction)
  * `close_after_frame` (window close path)
  * each of these also stamps `buffer.last_lost_focus_ms = app.frame_time_ms`
* `App.handleAfterSave` fans out to `Window.handleAfterSave` on every window; only `Maker` does anything — if it's in the `Running` state it clears its result buffer and respawns its build command (so saving triggers a rebuild)
* preview buffers (file_opener, buffer_opener, project_file_opener) construct buffers with `limit_load_bytes = true`, `enable_completions = false`, `enable_undo = false`, and are torn down and rebuilt whenever the selected entry changes; preview-only buffers are created directly with `initFromAbsoluteFilename` and `deinit()`'d locally (not added to the App buffer cache)
* status bar paints `style.emphasisRed` when `deleted_since_last_save` is set — the only visible indication of save state