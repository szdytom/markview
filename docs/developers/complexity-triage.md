# Cyclomatic complexity triage

The original `artifacts/complexity-triage.csv` lists 83 functions and closures above the maximum of 15. The table records their disposition in commit order, from low-risk local changes to larger coupled pipelines. Each item has its own formatted, linted and tested commit.

64 entries were refactored and 19 retain explicit function-level exceptions. The window-event extraction also marks three stateful input handlers, giving 22 measured exceptions in the completed source tree. Reasons live beside the functions and are printed by the audit; neither modules nor nested functions inherit an exception.

Run `cargo run -p xtask --locked -- complexity` to audit every production workspace target and platform module. Tests, test support, examples and benchmarks are excluded; macro bodies are not expanded. CI runs this command unconditionally in the checks matrix and its failure propagates to the aggregate `ci` job.

| CSV ID | Original function | Original score | Disposition | Commit |
| --- | --- | ---: | --- | --- |
| 70 | `src/fonts.rs` — `describe_one` | 16 | Refactored | [383a27b](https://github.com/szdytom/markview/commit/383a27b) |
| 83 | `xtask/src/icons.rs` — `run` | 16 | Refactored | [223cd33](https://github.com/szdytom/markview/commit/223cd33) |
| 27 | `crates/markview-layout/src/style/types.rs` — `name` | 17 | Explicit exception | [4cce2b8](https://github.com/szdytom/markview/commit/4cce2b8) |
| 29 | `crates/markview-layout/src/style/types.rs` — `color` | 17 | Explicit exception | [a94ddf8](https://github.com/szdytom/markview/commit/a94ddf8) |
| 57 | `src/app/settings_panel.rs` — `from_command` | 17 | Explicit exception | [54ba8f7](https://github.com/szdytom/markview/commit/54ba8f7) |
| 30 | `crates/markview-layout/src/style/types.rs` — `parse_paper_size` | 18 | Refactored | [3235388](https://github.com/szdytom/markview/commit/3235388) |
| 67 | `src/cli.rs` — `apply_reading` | 20 | Refactored | [ad1bc0b](https://github.com/szdytom/markview/commit/ad1bc0b) |
| 1 | `crates/markview-commonmark/src/html.rs` — `block_events` | 16 | Refactored | [2a0ffff](https://github.com/szdytom/markview/commit/2a0ffff) |
| 8 | `crates/markview-commonmark/src/parse.rs` — `merge_text` | 16 | Refactored | [72aaac6](https://github.com/szdytom/markview/commit/72aaac6) |
| 31 | `crates/markview-layout/src/style.rs` — `paint` | 16 | Refactored | [14d6508](https://github.com/szdytom/markview/commit/14d6508) |
| 50 | `src/app/launch.rs` — `run` | 16 | Refactored | [a63fb96](https://github.com/szdytom/markview/commit/a63fb96) |
| 68 | `src/cli.rs` — `finish` | 16 | Refactored | [d8fe575](https://github.com/szdytom/markview/commit/d8fe575) |
| 3 | `crates/markview-commonmark/src/html.rs` — `normalize` | 17 | Refactored | [11642bd](https://github.com/szdytom/markview/commit/11642bd) |
| 20 | `crates/markview-layout/src/search.rs` — `visit_fields` | 17 | Refactored | [b5deb2c](https://github.com/szdytom/markview/commit/b5deb2c) |
| 24 | `crates/markview-layout/src/style/parse.rs` — `validate_font_family` | 18 | Refactored | [aa6d401](https://github.com/szdytom/markview/commit/aa6d401) |
| 45 | `src/app/font_panel/view.rs` — `fonts_controls` | 25 | Refactored | [1bc7886](https://github.com/szdytom/markview/commit/1bc7886) |
| 56 | `src/app/pointer.rs` — `refresh_hover` | 28 | Refactored | [854c2d8](https://github.com/szdytom/markview/commit/854c2d8) |
| 26 | `crates/markview-layout/src/style/types.rs` — `chain` | 30 | Explicit exception | [9c8b270](https://github.com/szdytom/markview/commit/9c8b270) |
| 40 | `src/app/chrome/frame.rs` — `draw` | 31 | Refactored | [8332cf9](https://github.com/szdytom/markview/commit/8332cf9) |
| 78 | `src/net.rs` — `http_date` | 33 | Refactored | [0a83f4b](https://github.com/szdytom/markview/commit/0a83f4b) |
| 38 | `src/app/chrome/components.rs` — `draw_button_edges` | 39 | Refactored | [500bcea](https://github.com/szdytom/markview/commit/500bcea) |
| 47 | `src/app/fonts_command.rs` — `download` | 17 | Refactored | [7c8cc46](https://github.com/szdytom/markview/commit/7c8cc46) |
| 42 | `src/app/chrome/tabs.rs` — `draw_tabs` | 18 | Refactored | [b2c7517](https://github.com/szdytom/markview/commit/b2c7517) |
| 41 | `src/app/chrome/styles.rs` — `draw_styles` | 20 | Refactored | [d4fc593](https://github.com/szdytom/markview/commit/d4fc593) |
| 65 | `src/benchmark.rs` — `run` | 21 | Refactored | [80625e0](https://github.com/szdytom/markview/commit/80625e0) |
| 76 | `src/latency.rs` — `run` | 23 | Refactored | [8ed9063](https://github.com/szdytom/markview/commit/8ed9063) |
| 2 | `crates/markview-commonmark/src/html.rs` — `block_from` | 24 | Refactored | [20e8e42](https://github.com/szdytom/markview/commit/20e8e42) |
| 58 | `src/app/settings_panel.rs` — `update` | 25 | Explicit exception | [ca5a3d9](https://github.com/szdytom/markview/commit/ca5a3d9) |
| 46 | `src/app/font_panel/view.rs` — `draw_fonts` | 32 | Refactored | [173bc73](https://github.com/szdytom/markview/commit/173bc73) |
| 66 | `src/cli.rs` — `apply_command` | 32 | Refactored | [e3e8f98](https://github.com/szdytom/markview/commit/e3e8f98) |
| 39 | `src/app/chrome/components.rs` — `draw` | 33 | Refactored | [d436334](https://github.com/szdytom/markview/commit/d436334) |
| 23 | `crates/markview-layout/src/style/parse.rs` — `parse` | 40 | Refactored | [e8596cf](https://github.com/szdytom/markview/commit/e8596cf) |
| 25 | `crates/markview-layout/src/style/parse.rs` — `validate_field` | 55 | Explicit exception | [30b6819](https://github.com/szdytom/markview/commit/30b6819) |
| 61 | `src/app/text_input.rs` — `sync_input` | 16 | Refactored | [da0364a](https://github.com/szdytom/markview/commit/da0364a) |
| 73 | `src/images/source.rs` — `resolve` | 17 | Refactored | [d4a3490](https://github.com/szdytom/markview/commit/d4a3490) |
| 9 | `crates/markview-layout/src/fonts/validate.rs` — `table_tags` | 18 | Refactored | [db111b0](https://github.com/szdytom/markview/commit/db111b0) |
| 81 | `src/stylesheet.rs` — `install` | 21 | Refactored | [a3438d2](https://github.com/szdytom/markview/commit/a3438d2) |
| 79 | `src/security.rs` — `of` | 22 | Refactored | [bdbb4b7](https://github.com/szdytom/markview/commit/bdbb4b7) |
| 28 | `crates/markview-layout/src/style/types.rs` — `layout_relevant` | 32 | Explicit exception | [77bd16d](https://github.com/szdytom/markview/commit/77bd16d) |
| 18 | `crates/markview-layout/src/paginate.rs` — `paginate` | 16 | Refactored | [418fbad](https://github.com/szdytom/markview/commit/418fbad) |
| 19 | `crates/markview-layout/src/scene/flow.rs` — `seal_scene` | 16 | Refactored | [d5cea2f](https://github.com/szdytom/markview/commit/d5cea2f) |
| 52 | `src/app/lifecycle.rs` — `resumed 内初始化闭包` | 16 | Refactored | [e38156a](https://github.com/szdytom/markview/commit/e38156a) |
| 4 | `crates/markview-commonmark/src/incremental.rs` — `parse_incremental` | 17 | Refactored | [fe5548c](https://github.com/szdytom/markview/commit/fe5548c) |
| 17 | `crates/markview-layout/src/paginate.rs` — `prepare` | 17 | Refactored | [a4c4cf0](https://github.com/szdytom/markview/commit/a4c4cf0) |
| 59 | `src/app/single_instance.rs` — `start` | 17 | Refactored | [1301b03](https://github.com/szdytom/markview/commit/1301b03) |
| 77 | `src/net.rs` — `get` | 17 | Refactored | [30be27b](https://github.com/szdytom/markview/commit/30be27b) |
| 6 | `crates/markview-commonmark/src/parse.rs` — `html_events` | 18 | Refactored | [22edaf9](https://github.com/szdytom/markview/commit/22edaf9) |
| 10 | `crates/markview-layout/src/layout/blocks.rs` — `measured_block` | 18 | Refactored | [7c46df2](https://github.com/szdytom/markview/commit/7c46df2) |
| 34 | `crates/markview-pdf/src/paint.rs` — `math` | 18 | Explicit exception | [b968c70](https://github.com/szdytom/markview/commit/b968c70) |
| 15 | `crates/markview-layout/src/layout/table.rs` — `table` | 19 | Refactored | [a800fc9](https://github.com/szdytom/markview/commit/a800fc9) |
| 36 | `crates/markview-render/src/frame.rs` — `prepare` | 20 | Refactored | [807c7f8](https://github.com/szdytom/markview/commit/807c7f8) |
| 62 | `src/app/text_input.rs` — `input_event` | 20 | Refactored | [3c04451](https://github.com/szdytom/markview/commit/3c04451) |
| 32 | `crates/markview-layout/src/text/selection.rs` — `rebase_selection` | 21 | Refactored | [38616d5](https://github.com/szdytom/markview/commit/38616d5) |
| 13 | `crates/markview-layout/src/layout/inline.rs` — `units` | 22 | Refactored | [de270d9](https://github.com/szdytom/markview/commit/de270d9) |
| 21 | `crates/markview-layout/src/shaping.rs` — `resolve_fonts` | 22 | Refactored | [3f81148](https://github.com/szdytom/markview/commit/3f81148) |
| 12 | `crates/markview-layout/src/layout/inline.rs` — `prepare` | 23 | Refactored | [b5756fb](https://github.com/szdytom/markview/commit/b5756fb) |
| 71 | `src/images/cache.rs` — `fetch_cached` | 23 | Refactored | [16ee80a](https://github.com/szdytom/markview/commit/16ee80a) |
| 75 | `src/images.rs` — `schedule` | 23 | Refactored | [70f1f40](https://github.com/szdytom/markview/commit/70f1f40) |
| 43 | `src/app/chrome.rs` — `buttons_for` | 24 | Refactored | [89f778c](https://github.com/szdytom/markview/commit/89f778c) |
| 54 | `src/app/lifecycle.rs` — `tick` | 24 | Refactored | [55221ef](https://github.com/szdytom/markview/commit/55221ef) |
| 69 | `src/export_cli.rs` — `run` | 25 | Refactored | [3bf2683](https://github.com/szdytom/markview/commit/3bf2683) |
| 55 | `src/app/painting.rs` — `render` | 26 | Explicit exception | [9a17a67](https://github.com/szdytom/markview/commit/9a17a67) |
| 7 | `crates/markview-commonmark/src/parse.rs` — `child_block` | 27 | Refactored | [a3f109d](https://github.com/szdytom/markview/commit/a3f109d) |
| 48 | `src/app/gestures.rs` — `handle_touch` | 28 | Refactored | [cf11e81](https://github.com/szdytom/markview/commit/cf11e81) |
| 35 | `crates/markview-platform/src/window_frame.rs` — `subclass` | 29 | Explicit exception | [9f77e96](https://github.com/szdytom/markview/commit/9f77e96) |
| 33 | `crates/markview-pdf/src/paint.rs` — `draw` | 31 | Explicit exception | [1886bc5](https://github.com/szdytom/markview/commit/1886bc5) |
| 37 | `crates/markview-render/src/paint.rs` — `draw` | 31 | Explicit exception | [3803cb7](https://github.com/szdytom/markview/commit/3803cb7) |
| 80 | `src/settings/store.rs` — `flush` | 31 | Refactored | [8ea2715](https://github.com/szdytom/markview/commit/8ea2715) |
| 16 | `crates/markview-layout/src/linebreak.rs` — `optimize` | 33 | Explicit exception | [7f75122](https://github.com/szdytom/markview/commit/7f75122) |
| 64 | `src/app/window.rs` — `key_pressed` | 35 | Refactored | [7720c1f](https://github.com/szdytom/markview/commit/7720c1f) |
| 44 | `src/app/chrome.rs` — `overlay_with_form` | 39 | Refactored | [bbca681](https://github.com/szdytom/markview/commit/bbca681) |
| 72 | `src/images/decode.rs` — `decode` | 42 | Refactored | [fee82e6](https://github.com/szdytom/markview/commit/fee82e6) |
| 60 | `src/app/text_input.rs` — `key` | 43 | Refactored | [f251333](https://github.com/szdytom/markview/commit/f251333) |
| 82 | `src/worker.rs` — `with_services_and_parsed 内 worker 线程闭包` | 20 | Refactored | [3b99ed7](https://github.com/szdytom/markview/commit/3b99ed7) |
| 74 | `src/images.rs` — `prepare` | 28 | Refactored | [d6fbc36](https://github.com/szdytom/markview/commit/d6fbc36) |
| 22 | `crates/markview-layout/src/shaping.rs` — `shape` | 30 | Explicit exception | [0bc0ba4](https://github.com/szdytom/markview/commit/0bc0ba4) |
| 5 | `crates/markview-commonmark/src/parse.rs` — `inlines` | 36 | Explicit exception | [6ec59b5](https://github.com/szdytom/markview/commit/6ec59b5) |
| 51 | `src/app/launch.rs` — `run_with_options` | 39 | Refactored | [6205568](https://github.com/szdytom/markview/commit/6205568) |
| 11 | `crates/markview-layout/src/layout/blocks.rs` — `block_inner` | 40 | Explicit exception | [0833c5c](https://github.com/szdytom/markview/commit/0833c5c) |
| 53 | `src/app/lifecycle.rs` — `handle_user_event` | 59 | Explicit exception | [b1c1a7f](https://github.com/szdytom/markview/commit/b1c1a7f) |
| 14 | `crates/markview-layout/src/layout/paragraph.rs` — `paragraph_bounded` | 63 | Explicit exception | [701a571](https://github.com/szdytom/markview/commit/701a571) |
| 49 | `src/app/interaction.rs` — `action` | 70 | Explicit exception | [cc17c51](https://github.com/szdytom/markview/commit/cc17c51) |
| 63 | `src/app/window.rs` — `handle_window_event` | 163 | Refactored | [8e8cec5](https://github.com/szdytom/markview/commit/8e8cec5) |
