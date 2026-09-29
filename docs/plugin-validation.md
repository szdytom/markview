# Path B validation

This is the maintainer acceptance guide for the full preview extension,
`Stevvven.markview-export`. Markview4vsc combines the previously separate preview and export packages. Requirement closure is recorded in [plugin-requirements.md](plugin-requirements.md)
only after the review gate approves the evidence.

## Reproduce the installed-package test

Build the release engine, then run:

```sh
cd editors/vscode
npm ci
./package-vsix.sh darwin-arm64
MARKVIEW_VSIX="$PWD/dist/markview-export-darwin-arm64.vsix" ./run-tests.sh
```

The runner installs the VSIX into an isolated VS Code profile and uses the
bundled engine without an `enginePath` override. Success requires both
`MARKVIEW-EXT ok true` and `no engine outlived the window`. Temporary profiles
are removed when the runner exits.

## Manual workbench checks

Use a disposable VS Code profile and install the matching VSIX with
**Extensions: Install from VSIX…**. Open a local Markdown workspace, then run
**Markview: Open Preview to the Side**.

1. Edit the source without saving. Confirm the preview updates and retains its
   reading position. Scroll each pane and confirm the other follows without
   moving the source caret or oscillating.
2. Drag across preview text, including a paragraph boundary. Run **Markview:
   Copy Preview Selection** from its context menu and paste into a scratch
   editor. Confirm the text matches. Click a different word and confirm its
   source range is revealed.
3. Put the same distinct phrase near the beginning, middle and end of a long
   document. Focus the preview, open VS Code's native Find, enter the phrase,
   and use Next and Previous. Each off-screen match must bring its rendered
   paragraph into view. The automated
   `findInPreview` entry now invokes the same Chromium `window.find` call as
   VS Code and tests next/previous/wrap. This manual check additionally covers
   the workbench query field and keybinding.
4. Switch between light and dark themes. Text, code and Mermaid diagrams must
   recolor without changing paragraph positions. Change `markview.fontSize`
   and confirm the layout does change. Check different workspace folders and
   `[markdown]` overrides resolve independently.
5. Follow a relative Markdown link, a heading fragment and an HTTPS link.
   Markdown opens in the editor; the fragment reveals its target; HTTPS is
   forwarded to the browser. A non-Markdown local file requires confirmation.
6. Export a dirty buffer as PDF and PNG with a bundled template, a custom
   `.mvss.toml`, and explicit None while a default template is configured.
   Confirm the saved bytes reflect unsaved edits and the selected template.
   Scroll the preview afterward: export styling must not leak into it.
7. Close the window. Confirm its engine process exits. Repeat by terminating
   the extension host to establish crash cleanup.

## Performance boundary

The integration runner measures seven edits each at approximately 10 KB,
100 KB and 1 MB. Its timer starts at `TextEditor.edit`, then waits for visible
images to decode, two animation frames, and the host acknowledgment. Engine
wire bytes and webview JSON bytes are recorded separately; webview framing
is not counted. This is useful application latency evidence, but excludes
keyboard dispatch and physical display scanout.

A physical-key or focused workbench input run is still needed for NFR-1.
The workbench find input field remains undriven because the test host refuses
input focus. Its actual Chromium search and navigation path is covered through
the webview API, including off-screen matches and wrapping. Review approval is
still required; neither a test seam nor this document grants acceptance.

## Distribution

`Preview extension packages` produces six platform-specific artifacts on a
push, pull request or manual dispatch. Every package must contain the executable
for its own OS and architecture. Local packaging validates executable headers,
but an unexecuted CI matrix does not establish NFR-6. The workflow does not
publish to the Marketplace.
