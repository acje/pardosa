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

From 0.5.1 the ownership record carries an identity structure that holds across
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

The event envelope carries five fields this specification owns, and that set is
fixed at 1.0. The format admits a sixth field; this specification refuses to add
one.

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
fixed surface. At 1.0 that release is 1.89.0. A raise of that floor lands in any
minor release, and pardosa promises no window over which a given floor holds.

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

#### C5.44 — INVARIANT

pardosa requires no event kind standing for a fiber's detachment. An event's
envelope records whether the fiber it belongs to stood detached when that event
was written, and that record is the whole of what pardosa keeps on this axis. A
consumer holds pardosa to no second rendering of it.

#### C5.45 — INVARIANT

pardosa requires no event kind standing for a snapshot of accumulated state, for
a migration having occurred, or for a correction of an earlier event. A consumer
modelling any of the three declares it among the kinds of its own payload type
and carries its meaning itself.

#### The payload type and its descriptor

What a consumer's events are described by, who may produce that description, and
what identity it fixes.

#### C5.46 — INVARIANT

Every artefact carries a schema descriptor for the payload type its events hold.
The descriptor is present in every artefact pardosa writes, and pardosa admits no
artefact that omits it.

#### C5.47 — INVARIANT

An artefact's schema identity is a function of its schema descriptor. A change to
the described structure yields a different schema identity. Two schema identities
are compared for equality, and two schemas stand in a version relation when their
identities differ, which is the whole of what that relation records.

#### C5.48 — INVARIANT

A schema's identity comprises the described structure together with the names the
payload type and its nested types carry. The consumer supplies those names. A
name is a component of identity and never a key: pardosa keeps no registry of
names and settles no collision between two consumers that choose the same one.
Two payload types coincide in identity when they coincide in structure and in
name.

#### C5.49 — SURFACE

A schema descriptor is produced by the derive macro pardosa publishes for that
purpose and by the hand-written implementations pardosa ships within its own
crates. That set of producers is fixed at 1.0, and pardosa names the hand-written
implementations it ships. Admitting a further producer is an addition within a
major line.

#### C5.50 — INVARIANT

A payload type is an enumeration at its root, and each of its variants is one
event kind. Each variant carries an explicit discriminant, and the schema
descriptor records those discriminants. A structure stands as a variant's payload
and as a field's type, and stands as the root of no payload type.

#### C5.51 — SURFACE

A consumer's payload type declares the event kinds pardosa requires of it, and
that set is complete at one: the tombstone. The consumer writes the variant into
its own type and names it, and pardosa recognises the variant by the mark pardosa
defines for that kind rather than by the name the consumer chose. pardosa places
no variant into a consumer's payload type of its own accord.

#### C5.52 — INVARIANT

A migration removes a fiber whose latest event is a tombstone under the migration
policy that purges, and retains such a fiber under every other migration policy
this specification offers.

#### C5.53 — INVARIANT

Every path that yields events to a consumer compares the artefact's recorded
schema identity against the identity of the payload type the consumer names, and
does so on every adapter. A path yielding events without that comparison is a
path pardosa does not open. The conformance suite asserts this of each adapter.

#### C5.54 — INVARIANT

A reader yielding an artefact's records rather than its events makes no
comparison of schema identity, and this specification states that shape for it.
What that reader is given in place of the comparison is the artefact's schema
descriptor, from which it establishes the payload type for itself.

#### Naming

The rules governing every name pardosa teaches a consumer.

#### C5.55 — SURFACE

Every name pardosa gives a condition, a type or an item names the property that
holds, and names no mechanism by which pardosa established it. Where an
implementer needs the mechanism, the diagnostic an implementation renders carries
it.

#### C5.56 — SURFACE

Each concept pardosa teaches owns one word, and each word names one concept. The
word *schema* names a payload type's identity together with its description.

#### Membership, construction and release

Which dragline a fiber belongs to, how a consumer builds what pardosa exposes,
and what a release carries.

#### C5.57 — INVARIANT

An artefact's logical identity is the operator's naming of a dataset. The
operator supplies it, and it carries across a generation boundary unchanged. It
establishes nothing about whether the artefact has been tampered with.

#### C5.58 — INVARIANT

The rule partitioning fibers across the draglines of one logical identity is
declared by the operator. pardosa reads the declared rule and infers none. Where
a fiber falls outside the declared rule, pardosa refuses to open the artefact and
assigns that fiber to no dragline.

#### C5.59 — SURFACE

A public structure pardosa exposes is constructed through the constructors and
builders pardosa names for it. A consumer constructs such a structure by naming
one of those, and pardosa fixes no mechanism by which the restriction is held.

#### C5.60 — INVARIANT

pardosa publishes no release in which a clause binding from 0.5.1 is knowingly
unmet. Every adapter pardosa ships holds the exclusion this specification states
from its first published release, and the conformance suite asserts that
exclusion on each adapter it covers.

### Public surface

What the library exposes, what each type carries, and where each fact is recorded.

#### What the surface names

The vocabulary a consumer holds, and the entities pardosa teaches without publishing
a type for them.

#### C6.1 — SURFACE

The lifecycle types pardosa publishes are vocabulary a consumer names: a fiber's
state, the migration policy a caller selects, and the rescue policy governing a
locked fiber's migration. pardosa gives none of them a serialized form. A consumer
transporting one of these values across a boundary of its own supplies the
rendering itself.

#### C6.2 — INVARIANT

A fiber's locked state, a fiber's migrating condition, and a non-empty set of
removed fiber identities are reachable only within a running migration. An artefact
opened after a migration has finished yields none of the three, and pardosa states
this of each of them where it names them.

#### C6.3 — SURFACE

The classification a migration makes of each fiber it considers is pardosa's own.
What a consumer names of a fiber is its state and the policies the consumer selects
for it.

#### C6.4 — SURFACE

pardosa teaches *dragline* as vocabulary this specification defines. No public item
a consumer names is a dragline, and a consumer reaches every capability pardosa
offers without addressing one.

#### C6.5 — SURFACE

A dragline carries no identity a library consumer holds. The artefact's locator
names the dragline from outside, and the value a consumer compares between two
observations of one dragline is that dragline's rolling commitment.

#### C6.6 — SURFACE

An artefact's ownership record identifies the dragline that artefact is, and the
read-only operator interface reports that identity. An identity a consumer could
hold for a dragline enters no part of the surface a library consumer names. An
operator names a dragline; a library consumer does not.

#### What a caller receives

The types that carry pardosa's answers, and what each of them discloses.

#### C6.7 — SURFACE

pardosa answers three questions with three types. One names the condition under
which an operation failed. One is given to each sub-domain whose conditions are
closed by construction — the liveness verdict, the proof a verdict of death
carries, and whether an artefact is under migration — and enumerates that
sub-domain completely. One carries what pardosa knows about an artefact a caller
has just opened. A condition reached on a path that succeeded is carried by the
third and never by the first.

#### C6.8 — SURFACE

One type carries what a caller knows about the artefact it has just opened:
whether the artefact's generation is known, whether that generation is superseded,
which of the two migration disagreements holds, and whether a presented cursor
belongs to another generation. Its states are one complete enumeration, and a
caller reads them from the value it already holds rather than by choosing which
question to ask first.

#### C6.9 — INVARIANT

A failure pardosa reports carries diagnostic detail in a field pardosa owns. An
error type belonging to a storage backend is reachable from that failure by no
route pardosa provides, and a chain of causes a consumer walks reaches only types
pardosa names.

#### C6.10 — SURFACE

pardosa documents, for each condition it names, what a caller does next. The remedy
follows from the type the caller holds and from the condition that type names.
pardosa publishes no predicate over a condition.

#### C6.11 — SURFACE

The diagnostic detail a failure carries is the whole of what pardosa reports to an
operator about that failure. The surface fixed at 1.0 carries no further channel
for it.

#### C6.12 — SURFACE

pardosa offers one walk backward along a fiber's precursor chain. The walk ends at
the fiber's genesis event, and that ending is an ordinary one. A recorded precursor
outside the artefact and a recorded precursor belonging to another fiber each end
the walk under a name of its own.

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

#### What a migration takes, and what it leaves

The values a caller supplies to a migration, and what the resulting artefact
discloses.

#### C6.18 — SURFACE

The closure a caller supplies to a migration maps one payload value to another and
may refuse. It receives no event envelope and returns none. pardosa assigns every
field of the envelope in the artefact the migration writes.

#### C6.19 — INVARIANT

The caller names the rescue policy at each migration it starts, and names it at the
call that starts that migration. pardosa records the named policy with the
migration's record in the ownership record. No event of any artefact carries it.

#### C6.20 — INVARIANT

The artefact a migration writes holds the surviving fibers' events, re-identified
and chained densely from genesis. An event whose recorded precursor the migration
removed is a genesis event in the artefact written. The artefact written discloses
nothing about what the migration removed.

#### The schema descriptor and what it discloses

What describes a consumer's events, in what vocabulary, where it lives, and what a
reader may conclude from it.

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

#### C6.24 — SURFACE

The schema descriptor is the whole of the description pardosa publishes for a
payload type. pardosa publishes one item carrying that description and no second
rendering of it.

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
alone differ in identity, and the description a reader recovers from an identity
therefore includes the version that identity was computed over.

#### C6.28 — INVARIANT

A schema identity distinguishes one described structure from another and resists
accidental coincidence between two of them. A reader recomputes it from the
descriptor the artefact carries. It establishes nothing about who wrote that
descriptor and stands as evidence of no alteration.

#### C6.29 — INVARIANT

A reader meets two derived values and compares each. One is a function of the
schema descriptor, and moves with the payload type a consumer defines. The other is
a function of the event envelope's shape, which this specification fixes, and moves
with this specification. Neither is a function of the other's subject, and a
difference in either names the document whose subject changed.

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

#### What a ownership record carries about itself

The version the identity structure versions, and the reach of one artefact's
record.

#### C6.36 — INVARIANT

The version an artefact's identity structure carries is the version of that
structure's own shape. The generation an artefact belongs to is carried by the
chain of generation pointers, and the description of the events it holds is
carried by its schema descriptor. The identity structure's version carries neither
of those two facts.

#### C6.37 — INVARIANT

An artefact's ownership record carries the identity of the dragline that artefact
is, and carries the identity of no other dragline. A reader establishing which
draglines make up one logical identity reads the ownership record of each artefact
carrying that identity.

#### What each envelope field tells a consumer

The promise attached to each standard-owned field, and who reads it.

#### C6.38 — INVARIANT

An event's identifier is unique among the events of its artefact's current
generation and is assigned by pardosa when the event is committed. It promises
nothing across a generation boundary. An event's fiber identifier is unique within
the dragline holding that event.

#### C6.39 — SURFACE

A consumer reads an event's own identifier and its fiber identifier from the event
value it holds. The identity of the dragline that committed the event is reported
to an operator and is reached from no event value.

#### C6.40 — INVARIANT

An event envelope records whether that event is the one at which its fiber
detaches, and exactly one event of a detaching fiber carries that record. A
fiber's current condition follows from the events it holds taken in order. The
record states a fact about the fiber and states nothing about the entity a
consumer models with that fiber.

#### C6.41 — SURFACE

The discriminant each variant of a payload type carries is the consumer's own
value. pardosa reserves no discriminant, reads no meaning from any discriminant
value, and recognises each event kind it requires by the mark it defines for that
kind.

#### Taking authority to append

The operation by which a writer acquires the right to append, and what it reports.

#### C6.42 — SURFACE

pardosa offers one operation by which a writer takes exclusive authority to append
to an artefact. The operation grants that authority or names the condition
standing in its way, and names that condition in the vocabulary this specification
fixes. It takes no predicate from the caller and names no storage construct.

### Verification

The checks that hold the specified behaviour in place, and the strength of each.

#### C8.1 — INVARIANT

Where an adapter's durability step replaces the whole of an artefact's stored
content, that adapter compares the epoch the writer carries against the epoch
recorded for the artefact at every durability step. A durability step carrying a
superseded epoch is refused, the artefact stands as it stood, and the events
remain with the caller to present again.

#### C8.2 — INVARIANT

The conformance suite asserts that an artefact's schema descriptor is
structurally complete: the descriptor is present, every type reachable from the
payload type appears in it, every enumeration carries its explicit
discriminants, every bounded type carries its bound, and every constructor the
descriptor uses is one this specification defines. Structural completeness is
asserted on every adapter the suite covers.

#### C8.3 — INVARIANT

A consumer declares its payload type's schema version in the source that defines
that type. Across a migration the conformance suite asserts that the target
type's version stands later than the source type's, and a migration whose
version stands still or stands earlier is a conformance failure.

### Timing

The windows and waits the specified behaviour depends on.

#### C9.1 — INVARIANT

A migration whose source is under active append converges by transferring events
while the source continues to take them, and completes across a freeze window.
The window opens when the untransferred remainder is small, spans the transfer of
that remainder, and closes when the writer role passes to the application writing
the target. The source takes no append for as long as the window stands open.

### Artefacts

The on-disk structures, their topology, and what each one carries.

#### C10.1 — INVARIANT

An artefact's ownership record is itself an artefact of the kind pardosa manages.
It holds an ordered, append-only line of typed records, pardosa reads and writes
it through the machinery that reads and writes event data, and on every adapter
it offers the capability an ordinary artefact offers.

#### C10.2 — INVARIANT

Creating an ownership record and seeding the claim it carries is one indivisible
step. The step lands where no ownership record stands for the artefact and is
refused where one already stands, so exactly one of any number of concurrent
creators establishes the record.

#### C10.3 — INVARIANT

A writer holds exclusion on the artefact carrying the event data for the whole of
its writing session, and that exclusion is released when the session ends.
pardosa takes the exclusion the adapter offers and names each condition under
which the exclusion it took does not hold. What the exclusion establishes reaches
writers that take part in it.

#### C10.4 — INVARIANT

One ownership record stands for one artefact, and no ownership record spans two
generations. The artefact a migration writes carries an ownership record created
with it. A reader moving from one generation to the next follows the pointer the
record carries, and consults no artefact standing outside the two.

#### C10.5 — INVARIANT

An artefact's ownership record and its event data are held in one container
format, the ownership record's typed records being payloads of that format. On a
filesystem the two stand in one directory and share a stem, and the stem is
matched exactly, case included, on every platform.

#### C10.6 — INVARIANT

Every crate pardosa publishes carries, inside the published archive itself, the
full text of each licence under which that crate is offered.

### Vocabulary and constants

The fixed names, variant sets, and concrete values, and the meaning reserved for each.

#### Where this specification stands

The path holding the normative prose, and what that path names.

#### C12.1 — INVARIANT

This specification stands at `docs/spec/pardosa-1.0.md`. The path names the line
the document specifies rather than the version currently published, and each
major line is specified by a document at a path of its own, so a clause citation
resolves to the same clause for as long as that line stands.

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

Creating an artefact that already stands yields one condition, whatever moment
that artefact came to stand, and the caller's remedy is to open the artefact
instead. That condition is distinct from the condition a writer receives when its
claim to an already-standing artefact does not land.

#### C12.4 — INVARIANT

A write refused where a later owner has taken the artefact is a condition of
its own and carries one name on every adapter. It stands distinct from the
condition a writer receives when its claim does not land: a caller holding it
owned the artefact and owns it no longer, events it has already written may stand
in the artefact unreachable, and presenting those events again is unfounded.

#### C12.5 — INVARIANT

An artefact whose ownership record pardosa cannot establish is refused under one
name, and that name stands the same on every adapter. The name states that the
artefact's ownership is unestablished.
