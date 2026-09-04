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

#### C5.1 — INVARIANT

Every clause of this specification carries its axis as one of two markers, written
in the clause's own heading: INVARIANT for the axis binding from 0.5.1, SURFACE
for the axis fixed at 1.0. C4.1 states what each axis commits pardosa to and C4.2
states the question the axis answers. A clause carries exactly one marker, the
two markers are the whole of the set, and a clause's marker is read from its
heading and from nowhere else. A reader takes a clause's obligations from the
marker that clause carries.

#### C5.2 — INVARIANT

This specification is the whole of pardosa's normative commitment. Material
pardosa publishes elsewhere orients a reader towards the clauses here and adds
nothing to them, and a statement holds normative force through the clause that
carries it. Published material outside this specification carries no obligation
to cite a clause.

#### C5.3 — INVARIANT

A dragline admits one writer and any number of readers, on every adapter. During
a migration the source dragline keeps its own writer and the migration manager
reads it; the target dragline has the migration manager as its writer and is
readable throughout; at cutover the writer role passes from the migration manager
to the application. The migration manager takes exclusion by the mechanism the
adapter offers, and the exclusion a caller relies on is the same one every
pardosa writer relies on.

#### C5.4 — INVARIANT

pardosa fences the writer on every write path it opens, and offers no setting
that withdraws the fence.

#### C5.5 — INVARIANT

Every adapter's write path compares the epoch a write carries against the epoch
recorded for the artefact and rejects a write carrying a stale one. The rejection
happens where the write lands, and holding a claim record at the time of the call
is not what admits the write.

#### C5.6 — INVARIANT

Where the exclusion mechanism an adapter relies on is unavailable, pardosa
declines to open the artefact for writing and names that condition distinctly
from the condition where another owner holds the artefact. Read-only open takes
no exclusion and stays available on every target.

#### C5.7 — INVARIANT

A writer takes ownership by compare-and-set against the artefact's ownership
record, and the writer whose compare-and-set lands owns the epoch. A writer whose
compare-and-set does not land stops. Every write to the artefact carries the
epoch, and a write carrying an epoch a later owner has superseded is rejected on
every attempt.

#### C5.8 — INVARIANT

An artefact's ownership record carries its own ownership claim as its first
content, and nothing governs that record from above. This is the terminating case
of the ownership model. Every other artefact has its ownership established
through its ownership record, and no other artefact carries its own claim.

#### C5.9 — INVARIANT

An ownership record that exists and carries no claim is unowned. A writer takes
it through the ordinary compare-and-set path, and contention over it resolves the
ordinary way.

#### C5.10 — INVARIANT

pardosa creates an artefact's ownership record first and its event data second.
An ownership record present without its event data is an artefact under creation:
pardosa opens it, and the claimant or a later owner completes it. Event data
present without its ownership record is refused, and the refusal holds on every
path. pardosa repairs neither state.

#### C5.11 — INVARIANT

An artefact's ownership record and its event data are bound to each other by a
name the two share exactly. That binding is decided from the names alone, and
deciding it reads the contents of neither.

#### C5.12 — INVARIANT

Where pardosa cannot read an artefact's ownership record while fencing a write,
it refuses the write under a name distinct from the name it gives a stale-epoch
rejection. The refused write leaves the artefact as it stood and leaves the
events resident with the caller, and the caller may present them again.

#### C5.13 — INVARIANT

A safety mechanism pardosa relies on and cannot take is a refusal. Evidence that
would only make a decision cheaper, and whose absence leaves the fence intact, is
an indeterminate verdict. pardosa applies this distinction at every point where a
platform withholds something it depends on, and a platform withholding identity
evidence alone remains a full writer.

#### C5.14 — INVARIANT

pardosa establishes ownership takeover on proof. A record of clean release proves
release from any host. An owner that stopped without releasing leaves no proof,
pardosa reports an indeterminate verdict, and the takeover is the operator's to
initiate.

#### C5.15 — INVARIANT

pardosa names two conditions where a source and a target disagree about a
migration: the source announcing an outbound migration the target holds no record
of, and the target holding an inbound record the source announces nothing of.
Each carries its own name. The read proceeds under either, and pardosa selects no
winner between them.

#### C5.16 — INVARIANT

An outcome where a write may or may not have landed is an outcome of its own,
belonging neither to failure nor to success. A caller receiving it establishes
what landed before deciding, and a duplicate append is observable to that caller.

#### C5.17 — INVARIANT

A migration holds exclusive access to the artefact it writes. A caller that
starts one without that access receives a named failure. This specification
states the access a migration holds and leaves the mechanism by which an
implementation takes it unspecified.

#### C5.18 — INVARIANT

An operator initiates every migration. pardosa starts none of its own accord, on
any schedule or under any policy.

#### C5.19 — INVARIANT

An artefact under migration has exactly two generations, the current one and its
successor. A request to start a migration while one is running is refused under a
name the caller acts on.

#### C5.20 — INVARIANT

Every generation boundary mints fresh identity. Each migrated event receives a
new event identity and a new fiber identity, and no identity crosses the
boundary. A reference a consumer holds into a generation, a link between two
events, and a pointer from outside the artefact each address the generation that
issued it.

#### C5.21 — SURFACE

Migration tooling constructs through explicitly named constructors, on the same
terms as every other entry point pardosa publishes. A caller names what it is
constructing.

#### C5.22 — SURFACE

A resume cursor carries the identity of the generation that issued it. A cursor
issued in one generation and presented against another is rejected under its own
name.

#### C5.23 — INVARIANT

Two events are reordered when both are present in one dragline before and after a
migration and their relative order differs between the two. This is the whole of
what reordering means in this specification, and it is decided over pairs rather
than over sequences. Two events with no such pairing before a migration are
unconstrained after it.

#### C5.24 — INVARIANT

A migration preserves the relative order of every pair of events it retains in
one dragline, and pardosa holds this from 0.5.1 across every migration policy a
caller may select. The conformance suite asserts it.

#### C5.25 — INVARIANT

A fiber lives in exactly one dragline, and therefore in exactly one artefact. A
fiber spans no boundary between two of either.

#### C5.26 — INVARIANT

An artefact's rolling commitment establishes that the sequence it covers is
internally consistent and totally ordered, and pardosa holds that from 0.5.1.
Where an operator has wired an anchor destination, the artefact additionally
establishes that it has not been rewritten since an anchor an external observer
holds; that second establishment is a capability an operator elects. An artefact
with no anchor is unanchored, and unanchored is its own verdict rather than a
verdict of invalid.

#### C5.27 — INVARIANT

pardosa validates the fiber-scoped precursor chain on every adapter and on every
path that reads it. No setting, environment value or build option expresses the
absence of that validation.

#### C5.28 — INVARIANT

A discovered break in the precursor chain is a refusal on every ordinary path,
read and write alike, carried under a name of its own: it is neither a mechanism
pardosa could not take nor evidence pardosa lacks, and it stands as positive
evidence that the recorded data is wrong. No path delivers events from beyond a
discovered break to a consumer. The sanctioned entry into a broken artefact is an
election the caller names at the call site of the migration tooling.

#### C5.29 — INVARIANT

An anchor covers one artefact in one generation. An anchor an external observer
holds for a generation continues to establish that generation's artefact and
establishes nothing about an artefact a later generation produces. An operator
running an anchoring pipeline anchors again after each generation boundary.
pardosa states this obligation and does not check it.

#### C5.30 — INVARIANT

The vocabulary pardosa teaches a consumer is Pardosa-native throughout. A noun
belonging to a storage backend reaches no type a consumer names, no variant a
consumer matches, and no path a consumer imports.

#### C5.31 — SURFACE

pardosa's public surface is its type names and the full paths of its public
items. Diagnostic text an implementation renders is data an operator reads, and
an adapter's detail rides there without entering the surface.

#### C5.32 — INVARIANT

An artefact's schema describes the payload type taken whole. Where that type is
an enumeration, the schema covers the enumeration and the variants it names. An
artefact carries one payload type and therefore one schema, and every event in it
adheres to that schema. A consumer whose events differ in shape unites them under
one payload type.

#### C5.33 — INVARIANT

The conformance suite asserts the promise an adapter makes and asserts no
mechanism by which the adapter keeps it. An adapter reaching a stricter condition
than another while making the same promise conforms. Every symmetric promise this
specification states is an obligation the suite asserts.

#### C5.34 — INVARIANT

pardosa supports a backend when that backend's adapter passes the conformance
suite. The suite is published, and a third party establishes conformance for an
adapter by running it.

#### C5.35 — INVARIANT

pardosa refuses to open an artefact under a configuration that breaks an
invariant this specification promises. Where a configuration costs a property
this specification does not promise, pardosa opens and documents the cost. The
promise draws the line, and the strength of the configuration does not.

#### C5.36 — INVARIANT

The guard against corrupted event data is present in every build of pardosa. No
build option, feature selection or compilation choice removes it.

#### C5.37 — INVARIANT

pardosa's crates release in lockstep, and each depends on its siblings through a
range the just-published sibling satisfies. A published crate pins no sibling to
a single version, and a consumer resolves one version of each pardosa crate
across its graph.

#### C5.38 — INVARIANT

pardosa's compiler floor is the oldest stable Rust release that compiles the
fixed surface. A raise of that floor lands in any minor release, and pardosa
promises no window over which a given floor holds.

#### C5.39 — INVARIANT

pardosa publishes a security policy naming a contact and stating the process a
report follows, and commits to no response or remedy time. pardosa monitors the
advisories of its non-Rust dependency edge directly. A published release is
withdrawn for a correctness or safety defect and for nothing else.

#### Refusals

The clauses in this run state what pardosa is, and each names a reliance that
does not follow from it. A request for behaviour a clause here refuses is settled
by citing that clause.

#### C5.40 — INVARIANT

pardosa contracts the order of events within one fiber and contracts the
existence of one total order per artefact. It contracts no relation between the
events of two distinct fibers. A consumer holds pardosa to no cross-fiber order.
Where this specification describes what a dragline does across fibers, that
description records observed behaviour, and what binds is this refusal together
with the consumer's obligation under it.

#### C5.41 — INVARIANT

The fiber-scoped precursor chain establishes link integrity within one
generation: for two events both present in the artefact, the recorded predecessor
relationship stands as it was written. pardosa holds that from 0.5.1 and the
conformance suite asserts it. Three further readings do not follow from it. The
chain establishes nothing about completeness, and an artefact whose chain is
whole is consistent with events having been removed by a migration. The chain
establishes nothing about who wrote an event. The chain establishes nothing
across a generation boundary.

#### C5.42 — INVARIANT

An artefact's schema descriptor names the event kinds the payload type carries.
Naming the kinds that exist is the whole of what the descriptor offers, and
selection by kind does not follow from it. Selection by kind reaches across
fibers, and the affordance pardosa offers in its place is the same-fiber backward
window.

#### C5.43 — INVARIANT

Every commitment pardosa makes is held within one artefact. A consumer running
several artefacts holds one rolling commitment per artefact, each establishing
what it covers and nothing about its siblings. pardosa establishes no commitment
spanning two artefacts, and offers no aggregate over the commitments of several.

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
