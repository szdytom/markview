# Changelog

## 0.2.0

- Keep active-document following attached to the replacement engine after restarting preview.

- Combine native preview and PDF/PNG export as Markview4vsc, retaining the published identity and legacy settings/commands.
- Add binary tiles, stable repainting, shared templates, editor following and window restoration.


## 0.1.0

- Follow the active Markdown editor in the existing preview, preserving focus and discarding superseded tab changes.

- Restore open previews and reading positions after window reload; match outer spacing to the native template background.

- Send preview PNGs as binary frames and typed arrays instead of base64, preserving decoded-image swaps.

- Retain preview pixels while replacement tiles decode, then publish the complete visible update without a blank frame.

- Shrink wide previews to fit the pane with 16px outer spacing without changing template layout; keep selection and source synchronization aligned.

- Follow small scrolls in both panes with throttled alignment, without moving the caret; handle the document start and discard stale mappings.

- Share preview/export templates, retain template colors, and pin a distinctive Markdown toolbar icon.

- Add native pixel previews, source synchronization, selection, and template PDF/PNG export.
