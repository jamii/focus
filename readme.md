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

Scrolling on master (lib/focus/editor.zig):
* state lives on the editor as `top_pixel: isize` (which buffer pixel is at the top of the viewport), plus `wanted_center_pos: ?usize` for deferred centering requests
* mouse wheel: `top_pixel -= scroll_amount * yoffset` (scroll_amount = 32)
* drag past the top/bottom edge of the text rect nudges `top_pixel` by `±scroll_amount` per frame
* whenever the main cursor moves, `scrollPosIntoView` shifts `top_pixel` just enough to keep the cursor's wrapped line on screen (snap to top if above, snap to bottom if below)
* `scrollPosToCenter` / `scrollWrappedLineToCenter` set `top_pixel = max(0, center_pixel - text_rect.h/2)`; used by undo/redo, jump-to-pos, and after window resizes
* window-resize preserves the center: capture `getCenterPos` before updating `line_wrapped_buffer.max_chars_per_line`, then re-center on that pos afterwards
* alt+i / alt+k (buffer start/end) hardcode `top_pixel` directly so the view scrolls even when the cursor was already there
* each frame `top_pixel` is clamped to `[0, total_text_height - text_rect.h/2 - 1]` so you can always scroll the last line to roughly the middle but no further
* persistence: `buffer.last_center_pos` is written every frame and restored via `wanted_center_pos` in `Editor.init`, so reopening a buffer returns to the same view
* the left/right gutters draw `>` / `<` markers at a y proportional to `top_pixel` as a minimal scrollbar
* deferred application: `setCenterAtPos` only stores `wanted_center_pos`; the actual scroll happens at the top of the next `frame`, after line wrapping has been updated

Mouse interactions on master (lib/focus/editor.zig, events from lib/focus/mach_compat.zig):
* events come from GLFW callbacks wrapped as a tagged union: `mouse_motion`, `mouse_press`, `mouse_release`, `mouse_scroll`. buttons and mods are raw GLFW constants
* `mouse_motion` is emitted but the editor never reads it — drag tracking is done by polling instead (see below)
* pixel → buffer pos: `line = (top_pixel + (mouse_y - text_rect.y)) / char_height`, `col = (mouse_x - text_rect.x + char_width/2) / char_width` (the half-cell offset rounds to nearest column), then `line_wrapped_buffer.getPosForLineCol(line, col)` clamped to the last wrapped line
* `mouse_press` (left button, only when inside `text_rect`) branches on modifiers:
  * ctrl → `addCursor()` at pos, both head and tail set, `dragging = .CtrlDragging`
  * shift → extend current selection: `marked = true`, move only head, `dragging = .ShiftDragging`
  * none → `collapseCursors()`, `clearMark()`, move both head and tail, `dragging = .Dragging`
* every left-button press also calls `buffer.newUndoGroup()` so typing after a click is a separate undo
* `mouse_release` (left) just resets `dragging = .NotDragging`
* drag continuation does not use `mouse_motion`: while `dragging != .NotDragging`, the frame polls `glfwGetCursorPos` directly so dragging still works when the cursor leaves the window
  * each frame the dragged cursor's head is updated to the polled pos; if `cursor.tail.pos != pos and !marked` it calls `setMark()` so a drag implicitly starts a selection
  * if the mouse is past the top/bottom of `text_rect`, `top_pixel` is nudged by `±scroll_amount` per frame (auto-scroll while drag-selecting)
* `mouse_scroll`: `top_pixel -= scroll_amount * yoffset` (32 px per wheel notch)
* the completer popup is suppressed while any drag is in progress (`if (self.dragging != .NotDragging) break :completer`)
* clicks outside `text_rect` (gutters, status bar) are ignored — no cursor is created there
