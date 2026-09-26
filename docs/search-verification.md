# Document search verification

Find uses Ctrl/Cmd+F, Enter/Shift+Enter and F3/Shift+F3. Queries are literal,
ignore case by default, and use ICU word boundaries when **Word** is enabled.
Editing a query immediately submits a cancellable background search without
moving the reader or opening disclosures. IME preedit does not submit queries. Navigation wraps and opens only the target's ancestors.
Switching documents closes the search bar. Query, options and current result
belong to a tab and are retained when reopening search, but are not saved on exit.

## Automated checks

```sh
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p markview app::search::tests::search_bar_and_highlight_gpu_frames -- --ignored --nocapture
cargo test --release -p markview-core --test search large_document_search_measurement -- --ignored --nocapture
```

The GPU check writes `artifacts/search/*.png` for English, Simplified Chinese,
Traditional Chinese and Japanese, with light/dark palettes and 1×/1.25×/2× scales.
Integration tests cover semantic source projection, repeated blocks, hidden
content, query cancellation, stale results, file revisions, per-tab state,
wrapping navigation, horizontal positioning and selection independence.
Input tests cover preedit, candidate-confirming Enter, layered Escape and
unchanged Wayland candidate-window geometry.

A 5.5 MB document with 100,000 paragraphs now reports approximately 12.9 MB
of index allocations, down from 18.3 MB before optimization (about 30%). A
local release build measured 12 ms to build, 8 ms to return 100,000 matches
and 1 ms for a query with no matches. First-use whole-word segmentation for
a frequent one-letter query took 60 ms; subsequent queries reuse boundaries.
Timing is diagnostic, not a performance guarantee.

Search scans contiguous semantic text with field boundaries preserved and
checks cancellation between bounded chunks and matches. Query edits preserve
an index build for the same document version while cancelling stale matching;
a different document/version invalidates that build. Results arrive as a
shared ordered array, so the UI neither copies all matches nor allocates a
separate per-field highlight index. Visible blocks use binary search; clusters
within them are filtered individually to accommodate multiline image placeholders.

## Native GUI checklist

The native X11 check uses a separate settings directory, a 500×300 logical
window (2× desktop scale) and real keyboard events
for Ctrl+F, typing, Enter, F3, Shift+F3, repeated Ctrl+F and Escape. Its images
are `artifacts/search/native-compact.png`, `native-no-match.png` and
`native-closed.png`.

Complete these checks on the target desktop, particularly Wayland and macOS:

- [ ] Use a Chinese or Japanese IME in the search field. Preedit must not
  change the count. Enter must commit a candidate without navigating.
- [ ] Press Escape during composition: cancel preedit and keep the bar open.
  Press Escape again: close it, restore the status bar, keep the query.
- [ ] Confirm the candidate window follows the caret, including a long query
  scrolled inside the field. Leaving it idle must not cause a request loop.
- [ ] Resize to 500×300 and a wide window. Input, counts and all controls must
  fit; document selection and both scrollbar hit regions must end above the bar.
- [ ] Move the window between monitors with different DPI while the IME is
  active. Verify the caret, candidate window and pointer hits stay aligned.
- [ ] Search while scrolled halfway down a large file: typing must keep the
  position. First navigation should begin near the viewport; both directions wrap.
- [ ] Navigate into nested closed disclosures and a wide code block/table.
  Only ancestor disclosures open; the match start enters the visible region.
- [ ] Select and copy text while search highlights are visible. Copied content
  must match the selection, and closing search must leave that selection intact.
- [ ] Switch between two documents with different queries/options/results.
  Each switch must close the bar; reopening must restore that document’s query
  and options. Closing the active tab must leave the next document’s bar closed.
  Reload one from disk while typing; results must follow the new file only.
- [ ] Open Settings or Export while searching, then Ctrl/Cmd+F. The ordinary
  panel closes and the preserved query is selected. A confirmation dialog
  must retain keyboard and pointer priority.
- [ ] Close search while a status notification is still valid. Its remaining
  notification time should be visible again in the restored status bar.

Real IME candidate selection and moving between physical displays need human
verification; automated IME events and rendered scale variants do not replace it.
