These fixtures derive from the pinned Noto subsets in
`crates/markview-core/tests/fonts` and retain the license in
`licenses/Noto-OFL.txt`. They are test data, never package assets.

Generated with fontTools 4.66.1: load each original with `TTFont`, set
`font.flavor` to `woff` or `woff2`, and save. `Noto-subset.ttc` contains
Noto Serif Regular and Noto Sans Regular via `TTCollection`.

Coverage: CFF OpenType (Serif), TrueType/color bitmap tables (Emoji),
WOFF zlib, WOFF2 Brotli and an uncompressed collection. This is a regression
set, not a claim of full font-format or WOFF conformance.

The `Variable` and `VariableItalic` WOFF2 fixtures retain the `wght` axis
(100–900) from the Fontsource variable packages for Noto Serif, Sans and
Sans Mono at version 5.3.0. The CJK Serif Medium and SemiBold fixtures come
from `@fontsource/noto-serif-sc@4.5.12`, Chinese Simplified subset, weights
500 and 600. All retain the same OFL license above.

Sources use `files/<family>-latin-wght-<normal|italic>.woff2` in
`@fontsource-variable/<family>@5.3.0`, and
`files/noto-serif-sc-chinese-simplified-<500|600>-normal.woff2` in the static
CJK package. To regenerate, subset those files with fontTools using
`scripts/generate_test_fonts.py`'s `test_text()` plus the characters in
`web/apps/demo/src/documents.ts`, preserve every layout feature and name ID,
retain the `.notdef` outline, recalculate bounds and save with `flavor='woff2'`.
