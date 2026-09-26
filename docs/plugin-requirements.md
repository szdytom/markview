# Standalone VS Code export requirements

This submission extracts the export-only subset of the Path B editor work.
The native reader stays unchanged in purpose; this extension has no WebView,
preview panel, selection layer or editing surface. The full preview work is
outside this branch. Requirement IDs retain their original meanings below.

## Acceptance baseline

| ID | Requirement |
|:--|:--|
| INV-1 | Line breaking, microtypography, shaping, math, image handling, and stylesheet resolution happen in `markview-core` only. No second layout implementation may exist in the extension. |
| INV-3 | The editor owns the document text. The server exposes no method that mutates document text. |
| INV-4 | Plugin mode reads and writes no existing Markview user state: not `settings.toml`, not the user stylesheet directory, not the user font directory. All state lives under the extension's storage. |
| INV-5 | The extension works when Markview is not separately installed. |
| INV-6 | The webview owns no document, no layout, and no command surface. |
| ENG-1 | `--serve` runs a windowless server that needs no display and no `winit` event loop. |
| ENG-2 | A document's text can come from the client instead of the filesystem, and an unsaved buffer renders exactly as its bytes would from disk. |
| ENG-3 | A document keeps a path or base directory, so relative images and relative links resolve for a buffer that has never been saved. |
| ENG-12 | One `--state-dir` relocates the font directory, the stylesheet directory, and the image cache together. |
| ENG-13 | The process terminates when stdin reaches end of file, so an extension host that crashes or is killed cannot orphan it. |
| ENG-14 | Export is triggerable over the protocol and produces the same output as the existing CLI path for the same input. |
| ENG-16 | An export names its format and its template: the server renders PDF or PNG, layers a template the client names or the rules it holds, and lists the templates it can name. |
| EXT-8 | Every setting comes from VS Code configuration, resolved per document, including workspace-folder and language-scoped layers. |
| EXT-9 | Commands live in the command palette, menus, and user keybindings. The preview surface binds no keys of its own. |
| EXT-12 | Export commands run and reveal the result. |
| EXT-13 | The reader exports the document as PDF or PNG under a template of their choosing: one of the engine's own, or a `.mvss.toml` of their own that never has to be installed. |
| NFR-4 | No server process survives the VS Code session that owns it, under graceful quit and under a kill of the host. |
| NFR-5 | The cost of starting the engine is paid once per window, not once per document. |

## Verification and review gate

Run the locked Rust workspace checks, compile the extension, package the newly
built engine, and run the installed-VSIX tests. Close a requirement only after
`scripts/review-gate/review.sh` returns `approve`, using completed logs.
The review command is `codex review` with `gpt-6-astra` at medium effort.

## Progress

| Scope | Result |
|:--|:--|
| INV-1/3/4/5/6, ENG-1/2/3/12/13/14/16, EXT-8/9/12/13, NFR-4/5 | **Approve, round 4**, on main 67f1955. Round 1 interrupted for main refresh; round 2 caught template geometry, read-only sources, path headers, blocked EOF and SVG scaling; round 3 caught multi-tile SVG demand loss. Round 4 verified per-tile settling and found no remaining export regressions. 758 Rust tests and installed-VSIX tests pass. |

| Publisher preparation: INV-4/5, EXT-9/12/13, NFR-4 | **Approve**, first round, 2026-09-27. Manifest-derived installed-package discovery and PDF/PNG lifecycle tests pass under `Stevvven.markview-export`; native binary unchanged. |

| Display name: EXT-9/12/13 | **Approve**, 2026-09-27: **Better markdown PDF** across the manifest, command/settings titles and README; installed-VSIX tests pass. |

| README: EXT-8/9/12/13 | **Approve**, 2026-09-27: documented commands, scoped settings and MVSS authoring; linked guide verified and starter template exports PDF/PNG. |

## State of play

The export-only baseline passed the review gate on 2026-09-26; the registered publisher package passed its metadata review on 2026-09-27.
**Better markdown PDF** (`Stevvven.markview-export`) exposes PDF, PNG and template export commands, document-scoped
settings, and editor context-menu entries. The bundled private engine accepts
`open`, `close`, `styles` and `export` over JSON lines. The packaged target is
currently Apple Silicon macOS. No Marketplace publication is part of this PR.

## Known limitations and open questions

- Six-platform VSIX packaging and fresh-machine distribution testing remain future work.
- The user has registered publisher `Stevvven`; its installation identity is `Stevvven.markview-export`. Marketplace submission remains pending.
- Templates are native MVSS, not CSS; remote, virtual and untrusted workspaces are unsupported.
- Automated dialog-sequence tests mock VS Code pickers. Installed-VSIX tests
  exercise their export implementation with explicit destinations.
