## minimum viable editor

Editor:

- [x] Render monospace ascii
- [x] Cursor movement
- [x] Insert/delete text
- [x] Selections
- [x] Multiple cursors
- [x] Multiple editors sharing one buffer
- [x] Mouse interactions
- [x] Cut/copy/paste
- [x] Load/save
- [x] Soft wrap
- [x] Scrolling
- [x] Undo/redo
- [x] Status bar

Tools:

- [x] Open file
- [x] Search project files
- [x] Search open buffers
- [x] Search within buffer
- [x] Search within project files
- [x] Navigation stack
- [x] Runner
- [ ] Search errors

Language specific:

- [x] Rust
- [x] Python
- [x] Shell
- [x] Nix
- [x] Markdown
- [x] Highlighting
- [x] Comment/uncomment
- [x] Smart indent
- [x] Manual indent (esp python)
- [x] Paren matching
- [x] Formatting
- [ ] Completion UI

Language server:

- [ ] Squigglies
- [ ] Completions
- [ ] Actions

VCS:

- [x] Diff
- [x] Traffic lights in editor gutter
- [x] Revision picker

Testing:

- [x] Deterministic simulation testing
- [x] E2E fuzzing
- [ ] Replayable history of live sessions, for later debugging
- [x] Test that IO functions are not reachable from focus-core build
- [ ] Max out fuzzer coverage

Arch:

- [x] Daemonize

Perf:

- [x] Don't store undo for readonly buffers
- [ ] Test that fuzzer actions take at most linear time in buffer size
- [ ] Test that fuzzer actions never take longer than frame budget

## wishlist

- more text structure
  - https://matklad.github.io/2023/03/08/an-engine-for-an-editor.html
  - https://github.com/matklad/abont
  - structured forms rather than adhoc ui
  - eg code folding - surprisingly hard to support simply
  - in general want it to be much easier to add ui. not custom per task.
- terminal editor
  - ghostty helpful here?
  - https://matklad.github.io/2025/08/31/vibe-coding-terminal-editor.html
- undo tree
- lsp
- magit (https://matklad.github.io/2026/03/05/jj-lsp-followup.html)
- file tree / dired
- tentative ideas for agent integration
  - point agent at special comments, open magit diff, add review comments, run
    again
  - conversation tree?
