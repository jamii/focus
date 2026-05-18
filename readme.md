Wishlist:
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

Short-term roadmap:
* scroll
* load / reload / save
  * diff on reload
  * maintain scroll position
* mark
* mouse interaction