# Markview

A native Markdown reader that renders source text straight to the screen. This
glossary fixes the language of its scrolling domain, where a reader's hand and
the page it moves are reconciled through time.

## Language

### The hand's side

**Packet**:
One wheel event from a high-resolution device: a distance the hand has already
travelled, delivered late and in batches rather than as a continuous stream.
_Avoid_: tick, chunk, event (when the packet nature matters)

**Flush**:
A large packet the OS delivers after a silence, carrying the motion it
accumulated meanwhile — mid-gesture a flush bridges a batch gap, and after a
lift it carries the flick's inertia in one or few big packets.

**Detent**:
One whole notch of a classic mouse wheel, reported as a whole number. Not a
packet; a discrete request for one eased step.
_Avoid_: notch

**Stream**:
The run of packets while one hand motion lasts. A pause beyond the gesture
boundary ends it, and nothing else announces that end.

**Quiet**:
A stream whose packets have stopped arriving. Nothing distinguishes a hand
halted on the pad from one lifted mid-flight, so quiet only quickens the
momentum's decay; it never ends the stream outright.
_Avoid_: idle, silent

**Bridge**:
A same-direction packet that continues a stream across a silence — the flush
a batchy touchpad delivers after the motion it accumulated. Only a packet
against the stream's speed begins a new gesture.
_Avoid_: session, run

**Gesture**:
One hand motion between boundaries: a reported start/end phase where the
platform gives one, else a pause in the event flow.

### The page's side

**Received distance**:
The distance the packets have named in total: what the hand has already
travelled and the page must eventually show.
_Avoid_: target, pending scroll (implementation names)

**Owed distance (debt)**:
Received distance the page has not shown yet. The page may never show less
than this without new packets naming it.

**Lead**:
Distance the page has shown beyond the received distance, borrowed against the
speed the stream was carrying. Capped; never repaid by moving backwards.
_Avoid_: overshoot, momentum debt

**Momentum**:
The speed a live stream carries, read from the spacing of its own packets. It
rides the page across the gaps between packets, decays faster while quiet and
dies once nothing is owed.
_Avoid_: inertia (reserved for what the OS itself sends after a hand lifts)

**Spend**:
To end a momentum: the speed is discarded and the page keeps only where the
owed distance and any lead have put it. A speed that has died with nothing
owed is spent; so is one a reversing packet cancelled.

**Coast**:
Motion after the input has stopped: the page travelling on speed it already
had. Touch gestures coast from a release velocity; a wheel stream's coast is
its momentum's lead.
_Avoid_: glide, inertia

### Symptom vocabulary (issue #3)

**Contact stop**:
The hand halts on the touchpad without lifting. Packets stop; no signal
announces it; the page must stop with the hand.

**Dead stop**:
Motion ending in a single frame instead of easing to rest.

**Reversal jerk**:
A brief motion against the hand's direction between two same-direction
gestures, caused by a lead meeting a new gesture's first packet.
