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

#### C4.1 — INVARIANT

This specification carries two kinds of commitment, and every clause carries
exactly one of them. A commitment on the invariant axis binds from 0.5.1 and
pardosa holds it from that release onward. A commitment on the surface axis is
fixed at 1.0: within the 0.5.x line the shape it describes may change at a minor
release, and from 1.0 it changes only at a major release. A consumer reads a
clause's axis and knows which of the two it has been given.

#### C4.2 — INVARIANT

Every clause records its own axis, and the axis is a property of that clause
rather than of the section holding it. The axis answers one question: does
breaking this clause break a consumer. It answers nothing about whether the
clause's text may change. A clause therefore holds a binding commitment while
its content stays open, and a clause that enumerates may add to its enumeration
in any release without altering what it promises.

#### C4.3 — INVARIANT

Where a clause refuses — where it states that pardosa does not do a thing, and
that reliance on its doing so is unfounded — the clause's axis attaches to the
force of the refusal rather than to its removal. It states from when reliance is
illegitimate. A later release that offers more than a refusal withheld breaks
nothing a consumer was entitled to hold.

#### C4.4 — SURFACE

Within a major line, pardosa's public surface grows only by an addition a
consumer cannot silently mis-handle. Three additions meet that test: a cargo
feature that adds capability and removes none; a new public item; and the
opening of the seal on the trait an adapter implements. Nothing else is added
within a major line. A further variant of a public enumeration and a further
field of a public struct each fall outside the test.

#### C4.5 — SURFACE

A public enumeration's variant set is complete. A consumer matches every variant
the enumeration names and meets no further variant within a major line. An
enumeration admits further variants only where this specification designates it
an open domain and names the domain that is open. This specification designates
no enumeration open.

#### C4.6 — SURFACE

The variant set of pardosa's top-level failure enumeration is complete. A
consumer matches its variants exhaustively and carries no arm for a variant it
has not been given.

#### C4.7 — SURFACE

Every variant of a pardosa enumeration names one condition. No enumeration
carries a variant standing for the conditions the others do not name.

#### C4.8 — SURFACE

The migration policy a caller selects when it locks a fiber, and the rescue
policy governing what a locked fiber's migration preserves, are both part of the
public surface. Each is a complete variant set, and each variant is a choice the
caller makes.

#### C4.9 — SURFACE

A public struct's field set does not grow within a major line. A consumer
constructing a public struct names every field the struct carries.

#### C4.10 — SURFACE

The trait an adapter implements is sealed: pardosa names the implementations
that exist, and an implementation authored outside pardosa is not admitted. The
obligations an adapter meets are public, and a third party establishes for
itself that an adapter meets them. Opening the seal is an addition within a major
line; closing it again is a major change. The trait marking which backend an
artefact belongs to carries no method, and the trait governing how bytes reach
durable storage seals separately from it. The trait carrying the exclusion
obligations stays internal, and conformance is what holds an adapter to them.

#### C4.11 — SURFACE

The migration manager is a module of the pardosa crate. It is part of the public
surface fixed at 1.0, and the vocabulary a migration failure surfaces through is
fixed with it.

#### C4.12 — SURFACE

An artefact's metadata record has its shape fixed at 1.0. The operator interface
reading that record answers three questions: which owner holds this artefact,
whether that owner is provably dead, and which migrations ran under which rescue
policy. The record's event set as an interface, access to its individual fields,
and the abstraction beneath it stay internal and are not fixed.

#### C4.13 — SURFACE

The metadata record carries nine kinds of record, and that set is fixed at 1.0.
A record of a kind pardosa does not recognise is rejected.

#### C4.14 — INVARIANT

From 0.5.1 the metadata record carries an identity structure that holds across
any number of draglines: a logical identity distinct from the artefact's physical
locator, a version, the identifiers of the draglines that make up that identity,
and the rule partitioning fibers across them.

#### C4.15 — INVARIANT

The order a dragline establishes over its events, the per-fiber precursor chain,
and the dense re-chaining across a generation boundary bind from 0.5.1. How those
events are physically laid down is fixed at 1.0. A clause governing an artefact's
layout states which of the two halves it governs.

#### C4.16 — INVARIANT

An artefact holds exactly one dragline. A consumer relies on one rolling
commitment covering the whole of that artefact.

#### C4.17 — INVARIANT

Between migrations, the order a dragline establishes over the events of different
fibers holds. A migration may remove events, and the order surviving a migration
is a subsequence of the order preceding it. A migration does not reorder the
events within a dragline. pardosa documents this order as a dragline's default
behaviour and offers no contract over it: a consumer may observe it and may not
hold pardosa to it.

#### C4.18 — INVARIANT

The event envelope reserves one optional slot, which 1.0 leaves unused and
unexposed. pardosa offers no interface for reading or writing event metadata, and
an event does not name the dragline it belongs to. A consumer carrying metadata
of its own carries it within the event type it defines, and that type is the
extension point pardosa documents.

#### C4.19 — INVARIANT

An envelope whose recorded shape differs from the shape pardosa expects is
refused, on every path. pardosa does not compute whether one schema is compatible
with another. A schema change is a migration, and migration is what a consumer
reaches for in place of computed compatibility. This refusal holds throughout the
1.0 line.

#### C4.20 — SURFACE

The event envelope carries five fields the standard owns, and that set is fixed
at 1.0. The format admits a sixth field; the standard refuses to add one.

#### C4.21 — SURFACE

pardosa's public surface is five modules, and that count is fixed at 1.0.
Material that would otherwise mint a sixth module is placed in the module whose
concept already holds it.

#### C4.22 — INVARIANT

An artefact is read by the major line that wrote it. Across a major boundary the
operator links both major lines and copies the events through. pardosa states
that boundary and leaves the copying to the operator.

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
