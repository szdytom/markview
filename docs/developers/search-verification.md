# Document search verification

`/` while reading starts an empty search without using the document selection,
so `/foobar` searches for `foobar`. Ctrl/Cmd+F uses selected text or restores
the previous query. Find navigates with
Enter/Shift+Enter and F3/Shift+F3. Text inputs retain `/` as literal text;
panels, option lists and image viewers keep keyboard priority. Queries are literal,
ignore case by default, and use ICU word boundaries when **Word** is enabled.
Editing a query immediately submits a cancellable background search without
moving the reader or opening disclosures. IME preedit does not submit queries. Navigation wraps and opens only the target's ancestors.
Each tab keeps its search bar, query, match options and input focus across tab
switches. The browsing session saves the bar visibility, query and match
options, and restores them on startup; input focus is not restored after a restart.

## Automated checks

```sh
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p markview app::search::tests::search_bar_and_highlight_gpu_frames -- --nocapture
cargo test --release -p markview-layout --test search large_document_search_measurement -- --ignored --nocapture
```

The GPU check writes `artifacts/search/*.png` for English, Simplified Chinese,
Traditional Chinese and Japanese, with light/dark palettes and 1×/1.25×/2× scales.
Integration tests cover semantic source projection, repeated blocks, hidden
content, query cancellation, stale results, file revisions, per-tab state,
wrapping navigation, horizontal positioning and selection independence.
Input tests cover preedit, candidate-confirming Enter, layered Escape and
unchanged Wayland candidate-window geometry.

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
  Each switch must restore that document’s bar visibility, query, options and
  input focus. Restarting restores saved search state without restoring input focus.
  Reload one from disk while typing; results must follow the new file only.
- [ ] Open Settings or Export while searching, then Ctrl/Cmd+F. The ordinary
  panel closes and the preserved query is selected. A confirmation dialog
  must retain keyboard and pointer priority.
- [ ] Close search while a status notification is still valid. Its remaining
  notification time should be visible again in the restored status bar.

Real IME candidate selection and moving between physical displays need human
verification; automated IME events and rendered scale variants do not replace it.
