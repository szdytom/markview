# Editor font services

This private Node package supplies reusable font services for editor integrations.
It does not register a VS Code extension yet.

```sh
npm ci --prefix web/app/vsc-ext
npm --prefix web/app/vsc-ext run lint
npm --prefix web/app/vsc-ext test
```

`FontIndex` enumerates system fonts, the download cache and configured directories.
It reads internal and localized family names from fonts and TTC collections, caches
metadata by path/size/mtime, and resolves a template's ordered family candidates.
`statuses()` reports matches and download families; `paths()` includes the matched
family's faces from the preferred source. This checks family availability, not glyph
coverage or exact rendering equivalence. Unreadable font files are skipped.

`installFamily()` stages a validated download, replaces one family's cache directory
and records its declared license/source metadata. Failed or cancelled downloads keep
the prior installed family. Callers serialize installations for the same family.
`runNative()` transports stdin and JSON progress to/from a subprocess; cancellation
uses a cancellation marker followed by bounded process termination.

Tests use committed font fixtures and temporary directories. No internet downloads
or globally installed fonts are needed for deterministic matching tests.

`FontClient` wraps the native catalog/download protocol and snapshots custom
templates for batch operations. It requires a compatible `markview fonts` CLI;
scanning and cache operations remain usable without a native binary.
