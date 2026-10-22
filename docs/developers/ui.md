# Native UI message flow

The reader keeps its native vector and glyph drawing. `markview-ui-core` depends
only on `markview-geometry` and does not access windows, fonts, GPU objects, files,
clocks or workers. It contains the `Component` protocol, FIFO message scheduling
and widget interaction, and re-exports the shared `Rect` type.

```text
input → widget layout → typed page message → update → effect descriptions
                                              ↓             ↓
                                            model       host services
                                              ↓             ↓
                                         borrowed view ← completion message
                                              ↓
                                    layout → native drawing
```

`Component::update` mutates its model and returns `Update<Effect>`. The update
contains ordered effects and an invalidation level: `None`, `Paint` or `Layout`.
Layout invalidation includes painting. `Runtime::send` queues messages;
`Runtime::flush` processes them in order and combines invalidation. The host
executes effects only after updates return, then sends completions through the
same queue. `Component::view` uses a lifetime-bound associated type so views can
borrow model data.

`app/ui_model.rs` is the parent component. It routes Settings, Export and Fonts
messages into independent models. Settings owns its preview flag and reader
preferences; preview updates only invalidate painting. Page offsets live with
the page model: the settings page's list offset sits on `settings_panel::Model`,
and the export and stylesheet page offsets and the scrollbar drag sit on
`interaction.panels`. Reader settings return persistence, session
and document-reflow effects. Export settings return persistence effects without
changing reader settings. Both use the same style-priority reducer. Preferences
owns the runtime and the native settings store; the store remains a host service.

Fonts returns download, cancellation and folder-opening effects. The component
holds catalogue and progress data; `app/font_service.rs` owns cancellation tokens
and executes downloads through the shared services. Every progress and settlement
message carries a run version. The component rejects old messages after retry.
Catalogue and chooser completions also retain the existing resource-version
checks before entering the component. Document workers keep their document and
content-revision checks in their existing host adapters.

`app/widgets.rs` adapts Button/Form/List to the core protocol. A button is a
description with a stable `WidgetId`, message, rectangle and optional clip.
Catalogue rows use family or stylesheet IDs; tab-drawer rows use lifetime tab IDs
that distinguish origins sharing a path and survive renames and reordering.
Dropdown anchors omit their changing highlighted option from their identity.
Identity is local to the running UI and is not a persistence format.
Drawer and toolbar controls use separate identities because both remain painted
during drawer dismissal.

Commands are a sum of domain enums. The top-level `state::command::Command`
names only the domain; each domain enum holds the variants its owner handles.

| Domain | Enum | Variants |
|---|---|---:|
| Document | `DocumentCommand` | 11 |
| Settings | `SettingsCommand` | 28 |
| Export | `ExportCommand` | 14 |
| Chrome | `ChromeCommand` | 20 |
| Search | `SearchCommand` | 6 |
| Fonts | `font_panel::Command` | 8 |

`Command::widget_id` dispatches to the domain enum and hashes the outer domain
discriminant, the domain variant's discriminant and a `Debug` key. Hashing both
discriminants keeps `scope` unique across the flat command surface; some call
sites compare scopes alone. Identity is only stable within one run: nesting a
command in its domain changes the key's text, never the press-to-release
mapping. `settings_dependent` is the domain-level precheck that keeps a command
off a settings catalogue that is still loading.

Hover previews are transient chrome state, not a component: pointer motion arms
a fixed delay, the event loop's tick opens a shared card with the target's
wrapped title, detail line and optional page body, and the renderer composes
that page from the snapshot the tab already holds, at device resolution, through
a scaled render pass. The page already on screen is never repeated in its card,
so the active tab previews its title and path only. No screenshot or texture is
cached between frames; a new target only adds a `Target` arm and its card
content.

The transient status line is owned by `app/status.rs::Status`, which holds the
message, the error bit and the expiry deadline. The footer asks it for both
lines: `current` returns the trailing message while its deadline is live, and
`warning` returns a failure that outlived its deadline. A message-only write
keeps the deadline it found, so replacing a live line does not shorten it.
Other modules write only through the owner's methods; none of them touches the
fields.

The confirmation is a component, not an `InteractionState` field. `app/modal.rs`
owns the pending question in `modal::Model`; its `current` question is private,
the host opens or dismisses it with `Message::Open` and `Message::Dismiss`, and
drawing and input ownership read it through `interaction.overlays.modal()` and
`interaction.overlays.modal_open()`. Navigation paths dismiss it by sending a
message, so no host module writes the field.

`UiLayout` supplies visible controls, keyboard traversal and the measured form.
Painting uses these placed control rectangles; the same clipped rectangles answer
pointer input. A scrolling form keeps offscreen controls in keyboard order and
reveals a focused control before painting. `Interaction` owns focus, hover and
pointer capture by widget identity. A release activates the current message only
if it still hits the captured widget. Disabled controls block controls behind them
and are skipped by keyboard navigation. Touch first prefers an exact hit, then
the nearest expanded target, constrained by its clip. Menus, text input, document
selection and native window gestures retain their specialized state.

Run the pure protocol tests with `cargo test -p markview-ui-core`. Run reader UI
tests with `cargo test -p markview --lib app::`; on Linux, select Lavapipe as
described in [Rendering tests](rendering-tests.md) for the GPU and baseline tests.
