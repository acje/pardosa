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

### Documentation authoring — non-normative process note

Public documentation that uses *dragline*, including the docs.rs landing page,
must cite its specification definition in [C2.1](#c21--invariant) through the
documentation authoring process. A missing citation breaches that process
commitment, not the normative library contract. Judgement is recorded through
the existing documentation process, per release in its standing PAR ADR edited
in place, without a gate. This note is not a numbered clause and adds no
normative consumer guarantee.

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
fiber yields that series in that order. pardosa offers this capability to
consumers building projections or aggregates over its events. It owes nothing
further for that purpose at 1.0.

#### C3.10 — INVARIANT

pardosa reads an event's tombstone variant when it migrates an artefact, and on
no other path: not on append, not on replay, and not on open.

#### C3.11 — INVARIANT

This specification governs the shape of the event envelope and what that shape
promises. Whether a reader parses an artefact's bytes at all is governed by the
format specification that artefact was written to. An artefact records both, and
each answers its own question.

### Evolution and compatibility

What is frozen, what remains free to change, and who extends the system.

#### C4.1 — INVARIANT

This specification carries two kinds of commitment, and every clause carries
exactly one of them. A commitment on the invariant axis binds from 0.5.1 and
pardosa holds it from that release onward. A commitment on the surface axis is
fixed at 1.0: within the 0.5.x line the shape it describes is free to change at a
minor release, and from 1.0 it changes only at a major release. A consumer reads a
clause's axis and knows which of the two it has been given.

#### C4.2 — INVARIANT

Every clause records its own axis, and the axis is a property of that clause
rather than of the section holding it. The axis answers one question: does
breaking this clause break a consumer. It answers nothing about whether the
clause's text is free to change. A clause therefore holds a binding commitment
while its content stays open, and a clause that enumerates is free to add to its
enumeration in any release without altering what it promises. Every clause of this
specification is normative from 0.5.1, and a clause's axis names which commitment
that clause carries rather than whether it carries one.

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

The migration policy a caller selects when it locks a fiber, and the rescue
policy governing what a locked fiber's migration preserves, are both part of the
public surface. Each is a complete variant set, and each variant is a choice the
caller makes.

#### C4.8 — SURFACE

A public struct's field set does not grow within a major line. A consumer
constructing a public struct names every field the struct carries. The types
admit a further field; this specification refuses to add one.

#### C4.9 — SURFACE

The trait an adapter implements is sealed: pardosa names the implementations
that exist, and an implementation authored outside pardosa is not admitted.
Opening the seal is an addition within a major line; closing it again is a major
change.

#### C4.10 — SURFACE

The trait marking which backend an artefact belongs to carries no method. The
trait governing how bytes reach durable storage seals separately from it.
The trait carrying the exclusion obligations stays internal. Conformance holds
an adapter to those obligations.

#### C4.11 — SURFACE

The migration manager is a module of the pardosa crate. It is part of the public
surface fixed at 1.0. The vocabulary used to report migration failures is fixed
with it.

#### C4.12 — SURFACE

An artefact's ownership record has its shape fixed at 1.0. The operator interface
reading that record answers three questions: which owner holds this artefact,
whether that owner is provably dead, and which migrations ran under which rescue
policy. The record's event set as an interface, access to its individual fields,
and the abstraction beneath it stay internal and are not fixed.

#### C4.13 — SURFACE

The ownership record carries nine kinds of record, and that set is fixed at 1.0.
The nine are the ownership claim, the clean release, the migration start, the
migration end, the inbound pointer, the outbound pointer, the rescue-policy
choice recorded with the migration start, the identity structure, and the schema
descriptor. A record of a kind pardosa does not recognise is rejected.

#### C4.14 — INVARIANT

From 0.5.1 each artefact's ownership record carries an identity structure:
a shared logical dataset identity distinct from the artefact's physical locator,
the structure's version, that artefact's own dragline identifier, and the rule
partitioning fibers across the dataset's draglines. C6.37 states how a reader
establishes membership without a roster in any one record.

#### C4.15 — INVARIANT

Migration pairwise order-preservation under C5.24, the per-fiber precursor chain,
and dense re-chaining across a generation boundary bind from 0.5.1.
C3.8 states the separate replay-determinism promise. The physical arrangement of
those events is fixed at 1.0. A clause governing an artefact's
layout states which of the two halves it governs.

#### C4.16 — INVARIANT

Between migrations, the order a dragline establishes over the events of different
fibers holds. A migration is free to remove events, and the order surviving a
migration is a subsequence of the order preceding it. A migration does not reorder
the events within a dragline. pardosa documents this order as a dragline's default
behaviour and offers no contract over it: a consumer is free to observe it and
holds pardosa to none of it.

#### C4.17 — INVARIANT

The event envelope reserves one optional slot, which 1.0 leaves unused and
unexposed. pardosa offers no interface for reading or writing event metadata, and
an event does not name the dragline it belongs to. A consumer carrying metadata
of its own carries it within the event type it defines, and that type is the
extension point pardosa documents.

#### C4.18 — INVARIANT

An envelope whose recorded shape differs from the shape pardosa expects is
refused, on every path. pardosa does not compute whether one schema is compatible
with another. A schema change is a migration, and migration is what a consumer
reaches for in place of computed compatibility. This refusal holds throughout the
1.0 line.

#### C4.19 — SURFACE

The event envelope carries five fields this specification owns: `event_id`,
`fiber_id`, `detached`, `precursor`, and `precursor_hash`. That set is fixed at
1.0. C5.61, C6.38, C6.39 state the identifier and detachment promises. C5.40 states
the within-generation link-integrity promise of the precursor and its commitment.
The format admits a sixth field; this specification refuses to add one.

#### C4.20 — SURFACE

pardosa's public surface is five modules: `pardosa::store` for the runtime,
`pardosa::schema` for payload-type identity and description, `pardosa::encoding`
for the wire contract and value constraints, `pardosa::file` for the container,
and `pardosa::prelude` for re-exports that broaden none of those surfaces.
That count is fixed at 1.0.
Material that would otherwise mint a sixth module is placed in the module whose
concept already holds it.

#### C4.21 — INVARIANT

An artefact is read only by the major line that wrote it. Export across a major
boundary is the operator's responsibility.

#### C4.22 — SURFACE

The published feature set is `uuid`, `nats`, and `unstable-test-support`.
The default set is exactly `uuid`; changing that default set is a breaking
change. `uuid` adds integration implementations. `nats` selects backend
capability, not consumer vocabulary. `unstable-test-support` is off by default,
as is `nats`; its `unstable-` prefix excludes its surface from the freeze and
permits changes at a minor release. The same test-support feature on
`pardosa-nats` is forwarded when `nats` is enabled.

Features are public API. A stable feature may be added at a minor release and
removed only at a major release. Enabling a feature only adds capability: it
removes, narrows or reshapes no existing public item. Features never gate the
vocabulary a consumer uses or an invariant this specification promises.
`zstd`, `blake3`, and the derive capability are unconditional, not features.

#### Additional evolution commitments

#### C4.23 — INVARIANT

pardosa's compiler floor is the oldest stable Rust release that compiles the
fixed surface. At 1.0 that release is 1.89.0. A raise of that floor lands in any
minor release, and pardosa promises no window over which a given floor holds.

#### C4.24 — SURFACE

A schema descriptor is produced by the derive macro pardosa publishes for that
purpose and by the hand-written implementations pardosa ships within its own
crates. That set of producers is fixed at 1.0, and pardosa names the hand-written
implementations it ships. Admitting a further producer is an addition within a
major line.

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
carries it. Published material outside this specification carries no normative
obligation to cite a clause.

#### C5.3 — INVARIANT

A dragline admits one writer and any number of readers, on every adapter. During
a migration the source dragline keeps its own writer and the migration manager
reads it; the target dragline has the migration manager as its writer and is
readable throughout with C6.15's qualifications; at cutover the target's writer
role passes from the migration manager to the application, subject to C5.63's
source-retirement condition. The
migration manager takes exclusion by the mechanism the
adapter offers, and the exclusion a caller relies on is the same one every
pardosa writer relies on.
C5.39 states the separate refusal of reliance on cross-fiber ordering.

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
record. The writer whose compare-and-set lands owns the epoch. A writer whose
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
present without its ownership record is refused on every write path. Read-only
open follows C6.14, subject to the ordinary schema and integrity checks.
pardosa repairs neither state.

#### C5.11 — INVARIANT

An artefact's ownership record and its event data are bound to each other by a
name the two share exactly. The names alone determine that binding; neither
artefact's contents are read to determine it.

#### C5.12 — INVARIANT

Where pardosa cannot read an artefact's ownership record while fencing a write,
it refuses the write under a name distinct from the name it gives a stale-epoch
rejection. The refused write leaves the artefact as it stood and leaves the
events resident with the caller. The caller may resubmit after establishing
current append authority and determining which events already landed; this
permission does not extend to re-appending events already known to have landed.
No new session is required solely because the ownership record was unreadable.
This is permission for caller resubmission, not automatic retry. The stale-session
retry prohibition in C8.1 and C12.4 remains in force.

#### C5.13 — INVARIANT

A safety mechanism pardosa relies on and cannot take is a refusal. Evidence that
would only make a decision cheaper, and whose absence leaves the fence intact, is
an indeterminate verdict. pardosa applies this distinction at every point where a
platform withholds something pardosa depends on, and pardosa remains a full writer
on a platform that withholds identity evidence alone.

#### C5.14 — INVARIANT

pardosa establishes ownership takeover on proof. A record of clean release proves
release from any host. An owner that stopped without releasing leaves no proof,
pardosa reports an indeterminate verdict, and the takeover is the operator's to
initiate.

#### C5.15 — INVARIANT

pardosa names two conditions where a source and a target disagree about a
migration: the source announcing an outbound migration the target holds no record
of, and the target holding an inbound record the source announces nothing of.
Each carries its own name. The read proceeds under either, subject to the ordinary
schema and integrity checks and C6.15's qualifications, and pardosa selects no
winner between them.

#### C5.16 — INVARIANT

When it is undetermined whether a write landed, the outcome belongs neither to
failure nor to success. It is an outcome of its own. A caller receiving it
establishes what landed before deciding. A duplicate append is observable to
that caller.

#### C5.17 — INVARIANT

A migration holds exclusive access to the artefact it writes. A caller that
starts one without that access receives a named failure. This specification
states the required access but leaves the implementation's mechanism for
acquiring it unspecified.

#### C5.18 — INVARIANT

An operator initiates every migration. pardosa starts none of its own accord, on
any schedule or under any policy.

#### C5.19 — INVARIANT

An artefact under migration has exactly two generations, the current one and its
successor. A request to start a migration while one is running is refused under a
name the caller acts on.

#### C5.20 — INVARIANT

Every generation boundary mints fresh identity. Each migrated event receives a
new event identity and a new fiber identity, and neither of those identities
crosses the boundary. A reference a consumer holds into a generation, a link
between two events, and a pointer from outside the artefact each address the
generation that issued it.

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
caller is free to select. The conformance suite asserts it.

#### C5.25 — INVARIANT

A fiber lives in exactly one dragline, and therefore in exactly one artefact. A
fiber spans neither two draglines nor two artefacts.

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
items. Diagnostic text is data an implementation renders for an operator to read.
It carries adapter detail without making that detail part of the public surface.

#### C5.32 — INVARIANT

An artefact's schema describes the payload type taken whole. Where that type is
an enumeration, the schema covers the enumeration and the variants it names. An
artefact carries one payload type and therefore one schema, and every event in it
adheres to that schema. A consumer whose events differ in shape unites them under
one payload type.

#### C5.33 — INVARIANT

The conformance suite asserts the promise an adapter makes and asserts no
mechanism by which the adapter keeps it. An adapter conforms if it reaches a
stricter condition than another while making the same promise. The suite asserts
every symmetric promise this specification states.

#### C5.34 — INVARIANT

pardosa supports a backend when that backend's adapter passes the conformance
suite. The suite is published through `unstable-test-support`. C4.22 states that
feature's stability exception. A third party establishes conformance for an
adapter by running the suite.

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
caret range the just-published sibling satisfies. A published crate pins no
sibling to a single version. A consumer resolves one version of each pardosa
crate across its graph.

#### C5.38 — INVARIANT

pardosa publishes a security policy naming a contact and stating the process a
report follows. pardosa commits to no response or remedy time and monitors the
advisories of its non-Rust dependency edge directly. A published release is
withdrawn only for a correctness or safety defect.

#### Refusals

The clauses in this run state what pardosa is, and each names a reliance that
does not follow from it. A request for behaviour a clause here refuses is settled
by citing that clause.

#### C5.39 — INVARIANT

pardosa contracts the order of events within one fiber and contracts the
existence of one total order per artefact. It contracts no relation between the
events of two distinct fibers. A consumer holds pardosa to no cross-fiber order.
Where this specification describes what a dragline does across fibers, that
description records observed behaviour, and what binds is this refusal together
with the consumer's obligation under it.

#### C5.40 — INVARIANT

The fiber-scoped precursor chain establishes link integrity within one
generation: for two events both present in the artefact, the recorded predecessor
relationship stands as it was written. pardosa holds that from 0.5.1 and the
conformance suite asserts it. Three further readings do not follow from it. The
chain establishes nothing about completeness, and an artefact whose chain is
whole is consistent with events having been removed by a migration. The chain
establishes nothing about who wrote an event. The chain establishes nothing
across a generation boundary.

#### C5.41 — INVARIANT

An artefact's schema descriptor names the event kinds the payload type carries.
Naming the kinds that exist is the whole of what the descriptor offers, and
selection by kind does not follow from it. Selection by kind reaches across
fibers, and the affordance pardosa offers in its place is the same-fiber backward
window.

#### C5.42 — INVARIANT

Every commitment pardosa makes is held within one artefact. A consumer running
several artefacts holds one rolling commitment per artefact, each establishing
what it covers and nothing about its siblings. pardosa establishes no commitment
spanning two artefacts, and offers no aggregate over the commitments of several.

#### C5.43 — INVARIANT

pardosa requires no event kind standing for a fiber's detachment. The envelope's
`detached` field marks the event that detaches its fiber, as C6.39 states; it is
not a snapshot of the fiber's current state. A consumer holds pardosa to no
second durable rendering of that transition.

#### C5.44 — INVARIANT

pardosa requires no event kind standing for a snapshot of accumulated state, for
a migration having occurred, or for a correction of an earlier event. A consumer
modelling any of the three declares it among the kinds of its own payload type.
The consumer supplies its meaning.

#### The payload type and its descriptor

What a consumer's events are described by, who produces that description, and
what identity it fixes.

#### C5.45 — INVARIANT

Every artefact carries a schema descriptor for the payload type its events hold.
The descriptor is present in every artefact pardosa writes, and pardosa admits no
artefact that omits it.

#### C5.46 — INVARIANT

An artefact's schema identity is derived from its schema descriptor. A change to
the descriptor yields a different schema identity. Identity comparison establishes
equality or difference, not direction. The descriptor's version supplies the
ordering C6.26 states and is itself an identity input under C6.27.

#### C5.47 — INVARIANT

A schema's identity includes the described structure, the names the payload type
and its nested types carry, and the declared version. The consumer supplies those
names. A name is a component of identity and never a key: pardosa keeps no registry
of names and settles no collision between two consumers that choose the same one.
Two payload types coincide in identity when their descriptors coincide, including
structure, names and version.

#### C5.48 — INVARIANT

A payload type is an enumeration at its root, and each of its variants is one
event kind. Each variant carries an explicit discriminant, and the schema
descriptor records those discriminants. A structure can be a variant's payload
or a field's type, but cannot be the root of a payload type.

#### C5.49 — SURFACE

A consumer's payload type declares the event kinds pardosa requires of it, and
that set is complete at one: the tombstone. The consumer writes the variant into
its own type and names it, and pardosa recognises the variant by the mark pardosa
defines for that kind rather than by the name the consumer chose. pardosa places
no variant into a consumer's payload type of its own accord.

When a required variant is missing, the diagnostic names the missing kind and its
recognition attribute and shows a declaration the consumer can copy. This
requirement fixes the diagnostic's content, not its exact wording.

#### C5.50 — INVARIANT

A migration removes a fiber whose latest event is a tombstone under the migration
policy that purges, and retains such a fiber under every other migration policy
this specification offers.

#### C5.51 — INVARIANT

Every path that yields events to a consumer compares the artefact's recorded
schema identity against the identity of the payload type the consumer names, and
does so on every adapter. A path yielding events without that comparison is a
path pardosa does not open. The conformance suite asserts this of each adapter.

#### C5.52 — INVARIANT

A reader yielding an artefact's records rather than its events makes no
comparison of schema identity, and this specification states that shape for it.
What that reader is given in place of the comparison is the artefact's schema
descriptor, from which it establishes the payload type for itself.

#### Naming

The rules governing every name pardosa teaches a consumer.

#### C5.53 — SURFACE

Every name pardosa gives a condition, a type or an item names the property that
holds, not the mechanism by which pardosa established it. When an implementer
needs the mechanism, the implementation's diagnostic provides it.

#### C5.54 — SURFACE

Each concept pardosa teaches owns one word, and each word names one concept. The
word *schema* names a payload type's identity together with its description.

#### Membership, construction and release

Which dragline a fiber belongs to, how a consumer builds what pardosa exposes,
and what a release carries.

#### C5.55 — INVARIANT

An artefact's logical identity is the operator's naming of a dataset. The
operator supplies it, and it carries across a generation boundary unchanged. It
establishes nothing about whether the artefact has been tampered with.

#### C5.56 — INVARIANT

The rule partitioning fibers across the draglines of one logical identity is
declared by the operator. pardosa reads the declared rule and infers none. Where
a fiber falls outside the declared rule, pardosa refuses to open the artefact and
assigns that fiber to no dragline.

#### C5.57 — SURFACE

A consumer constructs a public structure pardosa exposes through one of the
constructors or builders pardosa names for it. The consumer names that
constructor or builder. pardosa specifies no mechanism for enforcing that
restriction.

#### C5.58 — INVARIANT

pardosa publishes no release in which a clause binding from 0.5.1 is knowingly
unmet. Every adapter pardosa ships holds the exclusion this specification states
from its first published release, and the conformance suite asserts that
exclusion on each adapter it covers.

#### Additional operating rules

#### C5.59 — SURFACE

Every variant of a pardosa enumeration names one condition. No enumeration
carries a variant standing for the conditions the others do not name.

#### C5.60 — INVARIANT

An artefact holds exactly one dragline. A consumer relies on one rolling
commitment covering the whole of that artefact.

#### C5.61 — INVARIANT

An event's identifier is unique among the events of its artefact's current
generation and is assigned by pardosa when the event is committed. It promises
nothing across a generation boundary. An event's fiber identifier is unique within
the dragline holding that event.

#### C5.62 — SURFACE

`create()` and `open()` are distinct, strict named constructors. Creation refuses
an artefact that already exists, as C12.3 states. Open refuses when no artefact
exists; it does not create one as a convenience. An ownership record present
without event data is instead the artefact-under-creation case C5.10 already
permits to open and complete. An event-data-only read follows C6.14.

There is no `open_or_create` constructor and no destructive re-initialisation
API. Re-initialisation is an operator action: remove the ownership-record and
event-data pair, then call `create()`. This is not permission for a durability
step to discard logical history; C8.1 governs physical durability replacement.

#### C5.63 — INVARIANT

A migration whose source is under active append converges by transferring events
while the source continues to take them, and completes across a freeze window.
The window opens when the untransferred remainder is small, spans the transfer of
that remainder, and closes when the writer role passes to the application writing
the target. The source takes no append for as long as the window stands open.

Cutover is successful only when pardosa has permanently retired the source
artefact's append authority. Retirement is a condition of declaring success,
not subsequent cleanup: pardosa rejects every later source append, whether
attempted through a writer held before cutover or a newly acquired source writer.
The retired source remains available for historical reads under the ordinary
schema and integrity checks and C6.15's generation reporting.

After interrupted or uncertain cutover, pardosa must establish which generation
has append authority before permitting renewed ordinary writes to either source
or target. While that authority remains uncertain, those writes remain blocked,
potentially indefinitely. Operator assertion alone is insufficient to establish
that authority. Absence of migration metadata does not establish source append
authority. This requirement leaves valid source chase writes and migration-manager
target writes under C5.3 intact, and does not prohibit operator initiation under
C5.14. Partial-target reads follow C6.15. Recovery mechanisms and the evidence by
which renewed append authority is established remain unspecified.

#### C5.64 — INVARIANT

Creation of an ownership record is exclusive against concurrent creators. It
succeeds where no ownership record stands for the artefact and is refused where
one already stands, so exactly one concurrent creator establishes the record.
Creation may be interrupted before the first claim is written; the unseeded
record is unowned and claimable under C5.9.

#### C5.65 — INVARIANT

A writer holds exclusion on the artefact carrying the event data for the whole of
its writing session. That exclusion is released when the session ends.
pardosa takes the exclusion the adapter offers and names each condition under
which the exclusion it took does not hold. What the exclusion establishes reaches
writers that take part in it.

### Public surface

What the library exposes, what each type carries, and where each fact is recorded.

#### What the surface names

The vocabulary a consumer holds, and the entities pardosa teaches without publishing
a type for them.

#### C6.1 — SURFACE

The lifecycle types pardosa publishes are vocabulary a consumer names:
`FiberState` with `Undefined`, `Defined`, `Detached`, `Purged`, and `Locked`;
`FiberMigrationPolicy` with `Keep`, `Purge`, and `LockAndPrune`, whose treatments
C12.2 states; and `LockedRescuePolicy` with `PreserveAuditTrail` and
`AcceptDataLoss`, the audit-preservation versus accepted-data-loss choice for a
locked fiber's migration, selected and recorded under C6.19.
pardosa gives none of them a serialized form. A consumer
transporting one of these values across a boundary of its own supplies the
rendering itself.

#### C6.2 — INVARIANT

A fiber's locked state, a fiber's migrating condition, and a non-empty set of
removed fiber identities are reachable only within a running migration. A reopened
artefact yields none of the three, and pardosa states
this of each of them where it names them.

#### C6.3 — SURFACE

The classification a migration makes of each fiber it considers, `FiberAction`,
is internal at 1.0.
What a consumer names of a fiber is its state and the policies the consumer selects
for it.

#### C6.4 — SURFACE

pardosa teaches *dragline* as vocabulary this specification defines. No public item
a consumer names is a dragline, and a consumer reaches every capability pardosa
offers without addressing one.

#### C6.5 — SURFACE

A dragline carries no identity a library consumer holds. The artefact's locator
names the dragline from outside. A consumer compares that dragline's rolling
commitment between two observations of it.

#### C6.6 — SURFACE

An artefact's ownership record identifies the dragline that artefact is, and the
read-only operator interface reports that identity. An identity a consumer could
hold for a dragline enters no part of the surface a library consumer names. An
operator names a dragline; a library consumer does not.

#### What a caller receives

The types that carry pardosa's answers, and what each of them discloses.

#### C6.7 — SURFACE

pardosa's answers fall into three families of types. The first names the condition
under which an operation failed. The second gives a separate type to each
sub-domain whose conditions are closed by construction — the liveness verdict,
the proof a verdict of death carries, and whether an artefact is under migration —
and each type enumerates its sub-domain completely. The third is one qualified-result
type carrying what pardosa knows about an artefact a caller has just opened. A
condition reached on a path that succeeded is carried by the third family and
never by the first.

The following register collects established conditions and their owning clauses.
It does not yet define an exhaustive top-level variant inventory; the completeness
commitment in C4.6 remains to be fully supplied in this draft. Coexisting facts
remain independently visible, subject to the constraints in C6.8. A descriptive
row without an identifier does not mint a variant spelling.

| Family | Established condition or members | Meaning and owning clause |
| --- | --- | --- |
| Operation failure | `StoreAlreadyExists` | Creation finds an artefact already present; open instead (C12.3). |
| Operation failure | `ConcurrencyConflict` | A claim on an existing artefact loses compare-and-set; the claimant stops (C5.7, C12.3). |
| Operation failure | Stale-epoch write rejection | The writer had ownership and lost it; distinct from claim loss, no retry (C12.4). |
| Operation failure | Ownership cannot be established on write open | Shared refusal across adapters, not refusal of a qualified orphan read (C12.5). |
| Operation failure | Ownership record unreadable while fencing | Distinct from known loss of ownership (C5.12). |
| Operation failure | Exclusion unavailable, migration exclusion absent, or migration already running | Distinct conditions under C5.6, C5.17 and C5.19 respectively. |
| Operation failure | Discovered chain break or uncovered partition membership | Refusals under C5.28 and C5.56 respectively. |
| Operation failure | `SchemaMismatch`, `EnvelopeMismatch` | Separate top-level conditions under C6.33; unestablished mismatch cause follows C6.34. |
| Artefact-pair failure | `ArtefactMismatch` | The pair does not belong together; do not open it. Which pairing check failed belongs in diagnostic detail. |
| Value-decoding failure | `ValueConstraintViolated { constraint: ValueConstraint }` | A value violates a bound or validity constraint, not a payload-schema identity condition. |
| Closed value sub-domain | `ValueConstraint`: `TooLong`, `Empty`, `NotReal`, `InvalidChar`, `InvalidUtf8` | The five value-constraint codes; none is a catch-all. |
| Closed liveness sub-domain | `ProvenDead { proof }`, `Indeterminate` | Proof of death or absence of proof, never a proof of liveness (C2.5). `DeathProof` carries the death proof. |
| Closed migration-mode sub-domain | Steady, migrating | Whether an artefact is under migration; reopened-state limits remain C6.2. |
| Qualified successful read | Generation known or unknown, superseded generation, either migration-disagreement direction; independently qualified history integrity, migration-result completeness and append authority | C6.8, C6.14, C6.15 and C5.15; not top-level failures. |
| Cursor condition | Cursor from another generation | Rejected under C5.22; the operation stage and relation to successful open remain unspecified here. |
| Indeterminate write outcome | Whether the write landed is unknown | Neither success nor failure; establish what landed before deciding (C5.16). |

The death-proof facts include machine reboot, process absence and process-id
reuse. Clean release proves release under C5.14. This register does not assign
unruled variant spellings. Independent visibility does not turn mutually exclusive
alternatives within a closed sub-domain into coexisting facts.

#### C6.8 — SURFACE

One type carries what a caller knows about the artefact it has just opened:
whether the artefact's generation is known, whether that generation is superseded,
which of the two migration disagreements holds, and whether a presented cursor
belongs to another generation. For a migration target it also carries the
independent integrity, migration-result completeness and append-authority
knowledge C6.15 states. These facts belong to one qualified-result family, with
closed, exhaustive alternatives and explicit unknowns. Coexisting facts remain
independently visible: a caller reads them from the value it already holds rather
than by choosing which question to ask first.

The facts compose subject to the existing invariants, not as an unrestricted
product of alternatives. A superseded generation and a directional migration
disagreement can coexist, and neither hides the other. Valid partial target
history may be readable with migration-result completeness and append authority
both unknown under C6.15; readability does not confer write permission under
C5.63. Combinations forbidden by the invariants are excluded: independent
visibility relaxes neither C5.28's ordinary-path refusal on a discovered chain
break nor C6.2's reopened-state limits.

This composition rule does not require a complete combination table. Concrete
Rust representation is left to 0.5.1 development, preserving the closed
alternatives and every required distinction. The operation stage at which a
foreign cursor is rejected and its relation to successful open remain unspecified.

#### C6.9 — INVARIANT

A failure pardosa reports carries diagnostic detail in a field pardosa owns. An
error type belonging to a storage backend cannot be reached from that failure
through any route pardosa provides. A consumer walking the chain of causes
reaches only types pardosa names.

#### C6.10 — SURFACE

pardosa documents, for each condition it names, what a caller does next. The remedy
follows from the type the caller holds and from the condition that type names.
pardosa publishes no predicate over a condition.

#### C6.11 — SURFACE

Diagnostic detail is all pardosa reports to an operator about a failure. The
surface fixed at 1.0 carries no further channel for that failure.

#### C6.12 — SURFACE

pardosa offers one walk backward along a fiber's precursor chain. The walk ends at
the fiber's genesis event as an ordinary ending. A recorded precursor outside the
artefact and a recorded precursor belonging to another fiber each end the walk
under a name of its own.

#### Reading an artefact and finding its generation

What a read reports, where each record is written, and who writes it.

#### C6.13 — INVARIANT

An artefact's ownership record carries the epoch. pardosa reads the epoch from that
record on every path that needs it, takes it from no coordinator outside the
artefact, and accepts none an operator supplies.

#### C6.14 — INVARIANT

A read-only open of an artefact whose ownership record is absent succeeds. pardosa
reports the artefact as unowned with its generation unknown, and yields the events
the artefact holds.

#### C6.15 — INVARIANT

Every read-only open reports which generation state holds for the artefact it
opened. Where the artefact's generation is superseded, pardosa names that state and
yields the artefact the caller named. pardosa opens no other artefact on a caller's
behalf.

For a migration target, pardosa qualifies the read independently by the integrity
established for the history read, completeness relative to the intended migration
result, and knowledge of append authority. Valid partial target history remains
readable, including after interruption, under the ordinary schema and integrity
checks. Integrity retains the scopes and limits of C5.26, C5.27, C5.28 and C5.40;
partial-history qualification does not relax those checks or their refusals.

Migration-result completeness is reported as known complete, known incomplete,
or unknown. The referent is the result required by the migration's selected
policies and payload transformation, not equality with the source's event count
or identities. A partial target need not be a contiguous source prefix.
Interruption alone, or absence of an outbound pointer, establishes neither completeness nor
incompleteness; where neither is established, pardosa reports unknown.

Append-authority knowledge independently identifies which generation holds that
authority when established, including whether the target being read holds it;
otherwise authority is reported as unknown. Readability, valid integrity and known
completion do not themselves establish append authority. Unknown authority permits
an otherwise eligible qualified read and leaves C5.63's ordinary-write admission
requirement intact. These qualifications are knowledge about the history, not
additional reopened lifecycle states under C6.2.

#### C6.16 — INVARIANT

A migration is recorded in the ownership records of both generations. The metadata
record of the artefact a migration writes carries the record of the incoming
migration, written by the migration manager. The ownership record of the artefact a
migration reads carries the pointer to the next generation, written by that
artefact's own owner. Each record is written by the owner of the artefact holding
it.

#### C6.17 — INVARIANT

The pointer to the next generation is written once that generation is complete. Its
presence establishes that the generation it names is complete, and a reader
following it reaches a complete generation without establishing that for itself.
This is sufficient evidence of migration-result completeness under C6.15, not a
requirement that every complete target have such a pointer. Append authority
remains independently qualified under C6.15 and governed by C5.63.

#### What a migration takes, and what it leaves

The values a caller supplies to a migration, and what the resulting artefact
discloses.

#### C6.18 — SURFACE

The caller-supplied migration closure maps one payload value to another and is
free to refuse. It receives no event envelope and returns none. pardosa assigns
every field of the envelope in the artefact the migration writes.

#### C6.19 — INVARIANT

The caller names the rescue policy in the call that starts each migration.
pardosa records the named policy with the migration's record in the ownership
record. No event of any artefact carries it.

#### C6.20 — INVARIANT

The artefact a migration writes holds the surviving fibers' events, re-identified
and chained densely from genesis. An event whose recorded precursor the migration
removed is a genesis event in the artefact written. The artefact written discloses
nothing about what the migration removed.

#### The schema descriptor and what it discloses

What describes a consumer's events, in what vocabulary, where it lives, and what a
reader concludes from it.

#### C6.21 — INVARIANT

An artefact's schema descriptor describes the payload type its events carry. This
specification fixes the event envelope and the arrangement of a dragline's events.
Each of the two describes its own half, and neither describes the other's.

#### C6.22 — INVARIANT

A schema descriptor carries the payload type's own structure; the enumerations
reachable from that type, each with the discriminant every one of its variants
carries; and the bound of every bounded value the type holds. It carries no
rendering of those values into bytes. The structure a descriptor carries is
finite, and the types reachable from it form no cycle.

#### C6.23 — SURFACE

A schema descriptor is written in the type constructors this specification names,
and that set of constructors is complete. Each constructor means what this
specification states it means. A reader decodes a descriptor by implementing the
constructors this specification names, and parses no programming language to do so.

The settled constructor families are integers described by width and signedness,
bounded vocabulary types carrying their maximum bound, enumerations carrying
explicit discriminants, structures carrying ordered fields, and `Option`.
This family list does not yet supply the complete constructor inventory or the
specific widths; those definitions remain outstanding for this draft.

#### C6.24 — SURFACE

The schema descriptor is all the description pardosa publishes for a payload type.
pardosa publishes one item carrying that description and no second rendering of it.

#### C6.25 — INVARIANT

Every artefact carries its schema descriptor once, in the artefact-scoped metadata
its adapter offers, and a reader finds it in a place it knows before it opens the
artefact. Each adapter meets this through the artefact-scoped mechanism available
to it, and the promise a reader receives is the same on every adapter.

#### C6.26 — INVARIANT

A schema carries a version, and two versions of one schema are ordered. A reader
compares them and establishes which of the two was declared later. That order holds
among the versions of schemas sharing one name, and pardosa states no relation
between the versions of two schemas carrying different names. Which schemas a
reader decodes does not follow from that order, and the order over an artefact's
generations is a separate order.

#### C6.27 — INVARIANT

The version is a field of the schema descriptor. Two schemas differing in version
alone differ in identity. The description a reader recovers from an identity
therefore includes the version used to compute that identity.

#### C6.28 — INVARIANT

A schema identity distinguishes one described structure from another and resists
accidental coincidence between two of them. A reader recomputes it from the
descriptor the artefact carries. It establishes nothing about who wrote that
descriptor and stands as evidence of no alteration.

#### C6.29 — INVARIANT

A reader encounters two derived values and compares each. One is a function of
the schema descriptor and changes with the payload type a consumer defines. The
other is a function of the event envelope's shape, which this specification fixes,
and changes with this specification. Neither is a function of the other's subject.
A difference in either identifies the document whose subject changed.

#### What pardosa discloses about itself

The claims pardosa makes, the platforms it names, and the relationship a consumer
enters.

#### C6.30 — INVARIANT

pardosa states each claim it makes about detecting alteration for one mechanism at
a time, and states the scope of that mechanism alongside it. pardosa states no
claim spanning two such mechanisms and offers no single sentence a reader carries
away in place of them.

#### C6.31 — INVARIANT

pardosa names the platforms on which it opens an artefact for writing, and that
naming is the list of platforms pardosa supports for writing. That list is aix,
cygwin, freebsd, fuchsia, hurd, illumos, linux, netbsd, openbsd and solaris,
together with Apple's platforms. pardosa publishes no list of platforms it
excludes. Where a platform's standing is unestablished, pardosa records it as
unestablished and claims it in neither direction. A read-only open is available
wherever pardosa builds.

#### C6.32 — INVARIANT

pardosa states the size of the group that maintains it. Triage of a report a
consumer files is best effort, and pardosa commits to no time within which a report
is answered.

#### When a mismatch is reported

The conditions a mismatch resolves to, and what pardosa establishes about each.

#### C6.33 — SURFACE

A difference between the payload type a consumer names and the payload type an
artefact was written with, and a difference between the event envelope this
specification fixes and the envelope an artefact was written with, are two
conditions pardosa names separately. Each stands at the top level of the failure
enumeration, and a caller reaches neither through a constructor shared with the
other. Each condition's name states which of the two subjects moved.

The payload condition is `SchemaMismatch`; the envelope condition is
`EnvelopeMismatch`. Neither name asserts corruption or a named standard revision.

#### C6.34 — INVARIANT

pardosa establishes for every artefact it opens whether a mismatch holds, and
reports that verdict on every path that compares. Which of the two conditions
holds is established from the artefact's own schema descriptor. Where that
descriptor does not yield the answer, pardosa reports the mismatch and states its
condition as unestablished.

#### What a descriptor's production establishes

The claim pardosa makes about where a descriptor came from, and the limit of that
claim.

#### C6.35 — INVARIANT

pardosa establishes that a schema descriptor was produced by one of the producers
this specification fixes. Whether a descriptor describes the events an artefact
holds faithfully stands outside what pardosa establishes, and pardosa states that
limit wherever it names what a descriptor gives a reader.

#### What an ownership record carries about itself

The version the identity structure versions, and the reach of one artefact's
record.

#### C6.36 — INVARIANT

The version an artefact's identity structure carries is the version of that
structure's own shape. The generation an artefact belongs to is carried by the
chain of generation pointers, and the description of the events it holds is
carried by its schema descriptor. The identity structure's version carries neither
of those two facts.

#### C6.37 — INVARIANT

An artefact's ownership record carries the identity of its own dragline and no
other dragline. To establish which draglines make up one logical identity, a
reader reads the ownership record of each artefact carrying that identity.

#### What each envelope field tells a consumer

The promise attached to each standard-owned field, and who reads it.

#### C6.38 — SURFACE

A consumer reads an event's own identifier and its fiber identifier from the event
value it holds. The identity of the dragline that committed the event is reported
to an operator. No event value exposes that identity.

#### C6.39 — INVARIANT

An event envelope records whether that event is the one at which its fiber
detaches, and exactly one event of a detaching fiber carries that record. A
fiber's current condition follows from the events it holds taken in order. The
record states a fact about the fiber and states nothing about the entity a
consumer models with that fiber.

#### C6.40 — SURFACE

The discriminant each variant of a payload type carries is the consumer's own
value. pardosa reserves no discriminant, reads no meaning from any discriminant
value, and recognises each event kind it requires by the mark it defines for that
kind.

#### Taking authority to append

The operation by which a writer acquires the right to append, and what it reports.

#### C6.41 — SURFACE

pardosa offers one operation for a writer to take exclusive authority to append
to an artefact. The operation grants that authority or names the condition
preventing it, using the vocabulary this specification fixes. It takes no
predicate from the caller and names no storage construct.

#### C6.42 — SURFACE

The three published crates are `pardosa`, `pardosa-derive`, and `pardosa-nats`.
The migration manager belongs inside `pardosa` under C4.11 rather than in a
fourth published crate.

#### C6.43 — SURFACE

The ownership record's format requires an operator label. Supplying that label
is optional for the caller: when the caller supplies none, pardosa derives a
default from the process. The field's encoding and wire shape belong to the
format specification under C3.4; access to ownership-record fields remains
governed by C4.12.

#### Additional public obligations

#### C6.44 — SURFACE

An adapter's obligations are public. A third party establishes for itself that
an adapter meets them.

### Verification

The checks that hold the specified behaviour in place, and the strength of each.

#### C8.1 — INVARIANT

Where an adapter's durability step replaces the whole of an artefact's stored
content, that adapter compares the epoch the writer carries against the epoch
recorded for the artefact at every durability step. A durability step carrying a
superseded epoch is refused, the artefact stands as it stood, and the events
remain with the caller. Their retention grants no renewed write authority: the
fenced session must not retry, as C12.4 states.

#### C8.2 — INVARIANT

The conformance suite asserts that an artefact's schema descriptor is
structurally complete:

- The descriptor is present.
- Every type reachable from the payload type appears in it.
- Every enumeration carries its explicit discriminants.
- Every bounded type carries its bound.
- The declared schema version is present.
- Every constructor the descriptor uses is one this specification defines.

The suite asserts structural completeness on every adapter it covers.

#### C8.3 — INVARIANT

A consumer declares its payload type's schema version in the source that defines
that type. Across a migration the conformance suite asserts that the target
type's version stands later than the source type's, and a migration whose
version stands still or stands earlier is a conformance failure.

#### C8.4 — INVARIANT

From 0.5.1, `deny(missing_docs)` is a build gate for pardosa's public items.
An undocumented public item fails the build.

### Artefacts

The on-disk structures, their topology, and what each one carries.

#### C10.1 — INVARIANT

An artefact's ownership record is itself an artefact of the kind pardosa manages.
It holds an ordered, append-only line of typed records. pardosa reads and writes
it through the same machinery as event data. On every adapter it offers the
capability an ordinary artefact offers.

#### C10.2 — INVARIANT

One ownership record stands for one artefact, and no ownership record spans two
generations. The artefact a migration writes carries an ownership record created
with it. A reader moving from one generation to the next follows the pointer the
record carries, and consults no artefact standing outside the two.

#### C10.3 — INVARIANT

An artefact's ownership record and its event data use one container format. The
ownership record's typed records are payloads of that format. On a filesystem,
the two reside in one directory and share a stem. The stem is matched exactly,
including case, on every platform.

#### C10.4 — INVARIANT

Every crate pardosa publishes includes the full text of each licence under which
it is offered, inside the published archive itself.

### Vocabulary and constants

The fixed names, variant sets, and concrete values, and the meaning reserved for each.

#### Where this specification stands

The path holding the normative prose, and what that path names.

#### C12.1 — INVARIANT

This specification is at `docs/spec/pardosa-1.0.md`. The path names the line the
document specifies, not the version currently published. Each major line has its
own specification path, so a clause citation resolves to the same clause for as
long as that line exists.

#### What a caller chooses for a fiber

The complete set of treatments a migration offers, and the word for each.

#### C12.2 — SURFACE

A caller starting a migration chooses, for each fiber, exactly one of three
treatments: the fiber is kept, and its events are carried into the target; the
fiber is purged, and its events are erased from the target; or the fiber is
locked and pruned, and the rescue policy the caller names governs what the target
retains of it. These three are the whole of the migration-policy vocabulary a
caller holds.

#### The conditions a writer is given a name for

Three conditions on the ownership path, each with the name a caller matches on
and the remedy that follows from it.

#### C12.3 — INVARIANT

Creating an artefact that already exists yields one condition, regardless of when
it was created. The caller's remedy is to open the artefact instead. That
condition is distinct from the condition a writer receives when its claim to an
existing artefact does not land.

#### C12.4 — INVARIANT

A write refused where a later owner has taken the artefact is a condition of
its own and carries one name on every adapter. It stands distinct from the
condition a writer receives when its claim does not land: a caller holding it
owned the artefact and owns it no longer, whether events it has already written
stand in the artefact unreachable is undetermined, and presenting those events
again is unfounded.

#### C12.5 — INVARIANT

The write-path refusal for an artefact whose ownership record pardosa cannot
establish carries one name, the same on every adapter. The name states that the
artefact's ownership is unestablished. It does not refuse the read-only open
C6.14 permits.
