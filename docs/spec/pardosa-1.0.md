---
title: pardosa 1.0
line: 1.0
specifies: the pardosa 1.0 line
document: normative
---

# pardosa 1.0

<!-- STATUS -->
C5.1 states the regime scheme and what each clause's marker obliges.
<!-- /STATUS -->

### Stance

The frame pardosa speaks from, and the posture every refusal below follows from.

#### C2.1 — INVARIANT

A dragline is the unit within which pardosa establishes a total order over the
events it commits and enforces per-fiber linearizability. It holds an ordered,
append-only event line — the sequence of events it has committed — together with
the fibers those events belong to. Every event a dragline commits enters its
event line at exactly one position and belongs to exactly one fiber. A dragline
is where writes are serialized, where durability is fenced, and where a rolling
commitment over its event line is maintained.

#### C2.2 — INVARIANT

An event kind is one variant of the event type a fiber carries. A consumer
modelling against pardosa enumerates its kinds as the variants of a single type,
and pardosa names kinds directly wherever this specification describes what it
does with them.

#### C2.3 — INVARIANT

pardosa states the semantics of each mechanism it provides in its own terms and
makes no regulatory-compliance claim. A consumer maps those stated semantics
onto the obligations that bind it, and holds the defence of those obligations
across its backups, its replicas and its deployments.

#### C2.4 — INVARIANT

pardosa's guarantees reach the pardosa layer and stop there. What a filesystem, a
stream broker, or the infrastructure beneath either does with storage pardosa has
released stands outside every promise this specification makes. pardosa states
what it establishes and declines to state what it cannot.

#### C2.5 — INVARIANT

pardosa separates proof from the absence of proof when it reports what it has
established about a recorded owner. A verdict of indeterminate states that no
proof was available, and pardosa reports it as itself and never as death.
pardosa establishes death where a proof exists, and establishes liveness in no
case.

### Scope

What the 1.0 line covers, and what stays outside it.

#### C3.1 — INVARIANT

Erasure is a property of the artefact a migration writes: that artefact holds no
event the migration erased. The artefact the migration read keeps what it held,
and its disposal rests with the consumer.

#### C3.2 — INVARIANT

An operation is a migration when it transfers an artefact's current state to a
next artefact, producing a generation boundary with fresh identity. Migration is
within pardosa's scope, and pardosa leaves an artefact that stays live unchanged
except by append. Deriving a further artefact alongside an artefact that stays
live — aggregate snapshotting — meets no part of this test and stands
permanently outside pardosa's scope.

#### C3.3 — SURFACE

Pruning of history ships at 1.0 through the migration policy a caller selects
when it locks a fiber for migration. Log rewrite is a migration under the test
this specification states and is reached through that same selection; the 1.0
surface offers no separate entry point for it. Compaction trigger, policy and
scheduling are unspecified at 1.0 and remain open to a later line.

#### C3.4 — INVARIANT

This specification governs what pardosa's artefacts mean and what they promise.
The byte-level encoding of a field and the wire shape of an artefact are fixed by
the format specification an implementation is built to.

#### C3.5 — INVARIANT

An artefact's ownership record and its event data move, copy and restore
together. pardosa states the behaviour a caller receives when one of the two is
present alone. pardosa promises no atomic movement of the pair and provides no
operation that performs it; atomicity over a filesystem, a backup tool or an
operator's command stands outside every promise this specification makes.

#### C3.6 — INVARIANT

pardosa carries no correlation or causation identifier of its own. Correlation
across an event's originating request is the province of W3C Trace Context, which
a consumer runs alongside pardosa and carries within its own payload where it
chooses. pardosa's event envelope carries the fields this specification names and
adds nothing on this axis.

#### C3.7 — INVARIANT

Distributing work across more draglines is distributing it across more artefacts.
pardosa establishes no division within an artefact, and a consumer seeking
further parallelism runs further artefacts. pardosa establishes no ordering
between two artefacts.

#### C3.8 — INVARIANT

An artefact holds one total order over the events it carries, and replaying it
yields that order every time. That order is fixed once written. pardosa states no
relation between the events of two distinct fibers, and states nothing about how
concurrent appends interleave.

#### C3.9 — INVARIANT

A fiber is an ordered series of events under one domain identity, and replaying a
fiber yields that series in that order. This is the affordance pardosa offers a
consumer building projections or aggregates over its events, and pardosa owes
nothing further for that purpose at 1.0.

#### C3.10 — INVARIANT

pardosa reads an event's tombstone variant when it migrates an artefact, and on
no other path: not on append, not on replay, and not on open.

#### C3.11 — INVARIANT

This specification governs the shape of the event envelope and what that shape
promises. Whether a reader parses an artefact's bytes at all is governed by the
format specification that artefact was written to. An artefact records both, and
each answers its own question.

### Evolution and compatibility

What is frozen, what remains free to change, and who may extend the system.

### Rules of operation

The invariants that bind every adapter, every caller, and every generation boundary.

### Public surface

What the library exposes, what each type carries, and where each fact is recorded.

### Verification

The checks that hold the specified behaviour in place, and the strength of each.

### Timing

The windows and waits the specified behaviour depends on.

### Artefacts

The on-disk structures, their topology, and what each one carries.

### Vocabulary and constants

The fixed names, variant sets, and concrete values, and the meaning reserved for each.
