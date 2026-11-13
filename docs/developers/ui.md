# Change the native UI

Use this guide for controls, settings and panels. The reader draws native vectors
and glyphs; `markview-ui-core` supplies pure state updates and interaction.

## Message and effect boundary

```text
input → placed widget → typed message → model update → effect description
                                           ↓                ↓
                                      borrowed view     host service
                                           ↑                ↓
                                           └── completion message
```

An update mutates its model and returns ordered effects plus `None`, `Paint` or
`Layout` invalidation. The runtime processes messages in FIFO order. The host
executes effects after updates return and sends completions through the same
boundary. Pure components do not read files, start downloads or consult clocks.

Settings, export and fonts own independent models. Reader changes can persist
preferences and request reflow; paper settings must not alter reader geometry.
Font jobs carry a run version so retry or cancellation can reject old progress.
Keep native resources and cancellation tokens in host services.

## Add or change a control

1. Add the message to its owning domain and handle it in that model/handler.
2. Return the necessary host effects and the least invalidation that is correct.
3. Give the widget an identity tied to its logical object, not its row index or
   current display text. Filtering and reordering must not redirect a pending click.
4. Place it through shared form/widget geometry so painting and pointer input
   use the same clipped rectangle.
5. Test the update, effect, focus and press/release behavior; inspect visual states.

Route commands through the common application action path. Keyboard and pointer
activation must receive the same modal, focus and gesture cleanup. Cross-domain
changes call the owner's operation instead of mutating its fields.

## Input and drawing invariants

A release activates only the captured widget when the release still matches it.
Disabled controls block hits behind them and are excluded from keyboard traversal.
Offscreen form controls remain in keyboard order; focusing one reveals it.
Touch prefers exact hits before expanded nearby targets, respecting clipping.

Draw overlays in the declared chrome layer order. An overlay must not paint a
second copy of underlying controls or accept input through a modal surface.
Document selection, text input, menus and native window gestures have specialized
state; adding a widget should not create a competing gesture owner.

A paint-only preview should not restart document layout. Geometry-affecting
changes must invalidate layout; closing or replacing a page must settle its
transient focus, capture and pending effects.

## Verify

```sh
cargo test -p markview-ui-core --locked
cargo test -p markview --locked --lib app::
```

Reader integration tests include GPU cases: use [native test setup](testing.md).
Run [visual baselines](rendering-tests.md) for changes to appearance. Check narrow
and wide forms, fractional DPI, disabled/hover/pressed/focused states, clipping,
keyboard reveal, modal priority and touch targets. A screenshot does not prove
that the painted control and its hit region agree.
