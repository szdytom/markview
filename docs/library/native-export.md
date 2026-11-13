# Integrate the native exporter

Use the native CLI when an editor needs PDF/PNG output or native font management.
Web components do not provide those services. Install a compatible `markview`
binary, invoke it as a child process and check its exit status. Consult
`markview export --help` and `markview fonts --help` for the complete flag surface.

## Resource authority

Buffers default to Untrusted. Preserve the source document's mode explicitly;
`--base-dir` supplies resolution context, never permission. Resource grants bind
the current content and target. See [export permissions](../users/export.md#resource-permissions)
for grant syntax. A multi-user conversion service needs its own sandbox and egress
policy; the personal reader's permissions do not provide service isolation.

## Exporting an editor buffer

`markview export` accepts Markdown on stdin, so an editor can export unsaved
changes without creating a Markdown file:

```sh
markview export --stdin --format pdf --output snapshot.pdf --base-dir . < document.md
markview export --stdin --format png --output whole.png --scale 2 < document.md
```

Buffers default to Untrusted. The host must preserve the document's trust using
`--document-trust trusted|untrusted` and may supply the same `--allow-local-image`
and `--allow-network` grants described in the export permission guide. A resource directory does not grant
trust.

`--base-dir` resolves relative images (default: the current directory).
The `pdf` and `export` commands share their template and reading options.
Repeat `--style` for bundled or installed PDF templates, highest priority first;
`--style-file` layers a custom MVSS file targeting `pdf` above them. Both output
formats use the template's page geometry. `--cjk-type SC|TC|JP|none` selects the
CJK convention (default: SC); `--paragraph-indent`, `--left` and `--no-hyphens`
control paragraph layout. `--font-size` is in logical pixels (default: 16,
range: 1–200); PNG `--scale` ranges from 0.1 to 8.

Repeat `--fonts` for font directories or `--font-file` for explicit font files.
Reader, file and buffer exports all use downloaded fonts by default.
`--ignore-system-fonts` excludes system and downloaded fonts; explicit files
also exclude both, while retaining any directories named with `--fonts`.
An invalid explicit font file or one over 64 MiB fails the job before replacing
an existing output.

Stdout contains one JSON object per line: `progress` events with a `phase` and
`fraction`, followed by a `done` event with PDF page/byte counts or PNG dimensions.
Errors go to stderr and return a nonzero exit status. A host can create the
path supplied by `--cancel-file` to request cancellation at export checkpoints.
Hosts that require cancellation to preserve the destination should export to
a temporary output, wait for successful completion, and then replace it.

### Markdown dialect for buffer exports

`markview export --stdin --markdown-flavor obsidian` uses the core reader's
supported Obsidian syntax. The default remains `gfm`; this command does not load
saved reader settings. `--highlight-marks` enables `==highlight==` in GFM, while
Obsidian always enables it. `--hard-breaks` treats single newlines as hard breaks
in either preset and is off by default. Grid tables are enabled by default in
both presets; use `--grid-tables false` to disable them. These options apply to
both PDF and PNG.

## Editor integrations

Editors need the effective template's font requirements, including families that
have no download source. `markview fonts catalog --destination ui|pdf` reports
those requirements as JSON, with template choices, family names, licenses and
source metadata. Use `--style ID` or `--file FILE.mvss.toml` to select a template;
custom files layer over the destination defaults and any selected named style.
This is distinct from `fonts list`, which reports declared downloadable families
and their local availability.

The existing download command also supports editor-managed storage and progress:

```sh
markview fonts download noto-serif --json --directory ./font-cache
markview fonts download --file custom.mvss.toml --destination pdf --json --directory ./font-cache
```

`--destination ui|pdf` downloads from the effective template instead of only the
selected file's declarations. Existing family selection, `--force`, `--dry-run`
and `--jobs` still apply. JSON mode emits `progress` followed by `done`, or a
`selection` for a dry run. Failures return a nonzero exit status with details on
stderr. A host may create the path given by `--cancel-file` to cancel a transfer.
Without these options, terminal output and the user font directory are unchanged.

Hosts should download to a staging directory and promote it after success, keeping
their existing cache on failure or cancellation. The VS Code client lives in
`web/app/vsc-ext/` and uses these same font commands.
