These fixtures derive from the pinned Noto subsets in
`crates/markview-core/tests/fonts` and retain the license in
`licenses/Noto-OFL.txt`. They are test data, never package assets.

Generated with fontTools 4.66.1: load each original with `TTFont`, set
`font.flavor` to `woff` or `woff2`, and save. `Noto-subset.ttc` contains
Noto Serif Regular and Noto Sans Regular via `TTCollection`.

Coverage: CFF OpenType (Serif), TrueType/color bitmap tables (Emoji),
WOFF zlib, WOFF2 Brotli and an uncompressed collection. This is a regression
set, not a claim of full font-format or WOFF conformance.
