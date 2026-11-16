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

## Scrolling

New native controls own a `markview-interaction::ScrollSurface`. Set its geometry
with `set_max` or `fit`, paint with `offset()`, and pass `ScrollRequest` through
input routing to `apply`. Create requests from the window's
`animations.scrolls`: `wheel` preserves the device's `WheelMode`, while `step`
and `to` ease keyboard and navigation requests. Embedded views carry the same
request in `ViewInput::Scroll`; the host applies `ViewAction::Scroll` to its
surface. Do not convert wheel input to an absolute position.

Applying a request registers motion with the shared clock and wakes the first
frame. The frame driver advances all registered surfaces; controls never add
individual animation hooks or maintain an animation list. The clock holds weak
references, so closing a popup or replacing a model releases its motion.
`jump_to` is reserved for restoration and dragged scrollbar positions;
`pan_by` follows pointer or native gesture motion that already supplies frames.
Both interrupt pending easing. The low-level `ScrollPhysics` engine remains available
for document sessions with incomplete layout bounds and the Web host's own clock.

## Tooltips

Icon buttons use their localized `label` as a tooltip. Set `Button::tooltip`
to override it or to add a hint to a text button; an empty override suppresses
the hint. Keep the text concise and describe the action.

`markview-ui-core::Tooltip` owns the delay and dismissal state, with time supplied
by the host. The native host waits 600 ms on hover and shows keyboard focus hints
immediately. Targets include widget identity, clipped bounds and text, so changes
invalidate old hints. Clicks, scrolling, Escape, focus loss and touch dismiss them.
An open dropdown suppresses hints until it closes.
The passive overlay adds no hit targets, uses panel colors and UI typography,
wraps long labels and aligns the text start with the control's horizontal center
(the center of an icon button's icon).
Window edges clamp that alignment; controls near the bottom flip the hint above
without changing its horizontal reading origin.

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
