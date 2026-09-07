# Semantic reconciliation — adopted definitions and planning readiness

Status: **NONNORMATIVE; four definitions HUMAN-adopted on ha3s at
2026-09-07 09:30; delivery under `pardosa-gudk.1` independently APPROVED in
`pardosa-aovc`. Commander `pardosa-gudk.2`: decision-complete and READY for
0.5.1 implementation planning.** Current administrative closure status is recorded
in bd (`pardosa-gudk.2` and `pardosa-jn1`), not in this report.
Earlier documentary preparation was APPROVED in `pardosa-j8i7`. Earlier bounded-discovery
delivery under `pardosa-15nq.1` was APPROVED in `pardosa-t5di`; `pardosa-7vrc`
covers the preceding report, not this update.
Complete descriptor semantics, implementation, publication readiness and the
1.0 freeze are not established.
Updated 2026-09-07 under `pardosa-gudk` / `pardosa-gudk.1`; the normative
specification carries the accepted definitions and the RULED trace records
supplementary HUMAN provenance. No constructor catalogue or API is adopted here.
The [specification](../pardosa-1.0.md) alone carries normative commitments
(C5.2). This report is planning evidence containing prototype observations; it
is not a clean-room implementation input and does not cross that wall.

## Evidence and reading rule

`S` means `docs/spec/pardosa-1.0.md`; `T` means
`docs/spec/trace/ruled-trace.tsv`. Numeric line references below describe the
pre-adoption baseline; clause identifiers remain current after inserted lines.
Both were inspected directly. Prototype references `G/...` mean
`../gh-report/crates/...`, as source-checked in **Corrected semantic orientation**
(`bd show pardosa-nsru`), read before earlier evidence. They are attributed
source observations, not fresh prototype reads or executed tests in this mission.
Earlier **Inventory observation** (`bd show pardosa-sdrh`) contributes candidate
details only subject to that correction register; **Oracle summary**
(`bd show pardosa-oqlf`) is not an independent correctness certificate.

HUMAN `.91` (2026-09-06 22:01) selects a curated constrained vocabulary
and derive guidance, with any-possible-type support an explicit non-goal.
It selects neither a float ballot nor the prototype catalogue. HUMAN `.91`
(2026-09-07 08:06) selects bounded discovery, disposing the inventory planning
gate. **Descriptor semantics** (`bd show pardosa-jn1.91`) records that acceptance;
**Remaining holds** (`bd show pardosa-jn1.89`) points to downstream semantic
completion in the existing map transfer register S3, not an unresolved ballot.

## 1. What is settled, and at what granularity

| Settled commitment | Exact limit of this conclusion | Current source |
| --- | --- | --- |
| Integer family by width and signedness | The admitted widths and full value domains are still owed. | S:1215–1219; T:177 (R127) |
| Bounded vocabulary carrying maximum bounds | Exact members, units, emptiness and admissible parameters are still owed. | S:1193–1219 |
| Explicit-discriminant enumerations; ordered-field structures; `Option` | Family membership is settled, not every constructor's complete meaning. Payload root is an enumeration; structures occur below it. | S:735–738,1215–1219; T:112,177 |
| Supported enum/structure/newtype composition | Supported construction and decode preserve specification-assigned constraints. Custom names or constructors do not automatically turn arbitrary application predicates into those constraints. | S:1208–1213 |
| Finite, acyclic, portable structural description | Every reachable type, discriminant and bound is described; readers parse no programming language. | S:1193–1206,1427–1439; T:176,195,212 |
| Published derive and named in-tree descriptor producers | Neither class is removed; actual shipped hand-implementation names remain owed. Production assurance is not faithful-event-description assurance. | S:355–366,1322–1327; T:125–126,198–199 |
| UUID integration feature | Integration is promised separately from its still-unsettled descriptor constructor and value semantics. | S:330–342 |

Finite vocabulary means finitely described constructors and rules, not a finite
number of payload values or a ban on parameterized bounds and finite composition.
Completing constructor meanings is semantic work; assigning bytes to an already
defined meaning is format work (C3.4, S:102–104). A Rust spelling alone proves
neither a new semantic constructor nor a strategic exclusion.

## 2. Finite grouped discovery catalogue — not block adoption

These groups provide a bounded baseline envelope to investigate as a whole.
Domain examples are illustrative, **not established product requirements**.
Every row distinguishes an observed prototype meaning from an admitted standard
meaning. No row recommends admitting all its candidates.

| Natural-domain use and candidates | Observed meaning / candidate boundary | Semantic completion still needed |
| --- | --- | --- |
| Counts and signed quantities: fixed-width integers | Prototype inventory has unsigned and signed 8, 16, 32, 64 and 128-bit types. Their ordinary integer ranges are `0..=2^w−1` and `−2^(w−1)..=2^(w−1)−1`, respectively. `G/pardosa-schema/src/genome_safe.rs:42`; observer cites wire `primitives.rs:3–43`. | Select admitted widths and domains; domain units and restrictions such as percentages are not implied by an integer or a newtype name. |
| A two-valued fact: boolean | `bool` is in the prototype's trait inventory (`genome_safe.rs:42`), omitted by the earlier candidate census; its value alternatives are false and true. | Whether and how this scalar belongs to the standard constructor inventory. |
| Labels: bounded text, optionally nonempty | `EventString<MAX>` checks UTF-8 **bytes**, not characters; length `0..=MAX`. Thus `é` needs two bytes; empty text fits `MAX=0`. `NonEmptyEventString<MAX>` adds nonemptiness: no valid value at zero maximum. Sources: corrected orientation, `bounded.rs:26–59`; observer `:218–286`. | Text membership, units, empty/nonempty alternatives, maximum parameter domain and treatment of uninhabited parameterizations. A nameable Rust type is not a valid inhabited domain. |
| Opaque data and repeated items: bounded bytes / sequences | `EventBytes<MAX>` bounds bytes; `EventVec<T, MAX>` bounds **items**. Observed empty-through-maximum construction includes zero maximum. A count bound is not an aggregate byte bound or proof of inner validity. `bounded.rs:26–207`; maximum enters identity at `:304–322`. | Exact members and bound semantics, admissible element domains, parameter limits and preservation on every supported route. No maximum numeric parameter or aggregate resource budget is invented here. |
| Optional facts, records and alternatives | `Option<T>`, ordered products, explicit-discriminant sums and newtypes compose admitted inner meanings. Prototype arrays have a fixed item count; tuples have implementations for arities 1–16 (`genome_safe.rs:58–100`). | Arrays/tuples remain candidates; a tuple may use an existing product meaning rather than introduce another constructor. Newtype constraints need explicit semantics, not just private fields. Discriminant width and parameter admissibility remain semantic deliverables; `repr(u8)` is not adopted by this report. |
| A character, a time, an identifier | Character / `CharScalar`, `Timestamp`, UUID are prototype candidates. Observer reports Unicode-scalar checking (`char_scalar.rs:5–58`), nonzero `u64` nanoseconds since Unix epoch (`pardosa-wire/src/validate.rs:25–69`), and arbitrary 16-byte UUIDs without version/variant validation (`pardosa-wire/src/foreign.rs:12–23`, citation corrected by independent review `pardosa-7vrc`). Corrected orientation confirms Timestamp's payload trait implementation (`genome_safe.rs:47–50`), not just envelope use. | Each exact domain, epoch/unit or identifier restriction needs disposition if admitted. UUID feature support is already promised, but does not decide those meanings. These observer details are not newly executed boundary proofs. |
| Measurements and exceptional measurement results: three float-wrapper families | Corrected source: `RealF*` / `OrderedF*` checked paths admit **normal finite values plus zero**, reject subnormals/NaN/infinities, normalize negative zero (`floats/mod.rs:7–24`). `EventF32/F64` have `NaN`, `NegInf`, `Finite(OrderedF*)`, `PosInf`; classification still fails on finite subnormals (`floats/event.rs:20–31,51–60,125–136,148–157`). | No float family is adopted or permanently excluded. Decide membership and all exceptional cases only within an authorized completion envelope. No float-vs-fixed-point choice, wire representation or total-IEEE-classification claim is made here. |

Evidence limits matter to the envelope. The prototype's generated newtype decode
directly assembles the field (`G/pardosa-derive/src/codec.rs:169–178`, corrected
orientation). A hypothetical `Percent(u8)` with a checked `0..=100` constructor
therefore does not establish that derived decode preserves that predicate.
This is a construction-route counterexample, **not a requirement to admit a
percentage constructor**. Likewise, `Real`/`Ordered` public transparent serde
derives are alternate routes not certified by their checked constructors;
`EventF` itself has no serde implementation in the inspected module. No fresh
serde proof or exhaustive downstream-route census was executed here.

Candidate discovery envelope, not an adopted catalogue: investigate a small set
of domain-useful scalar meanings, bounded
text/bytes/item collections and supported composition, with exact constraints
and portable descriptors. Escalate a materially new family or constraint meaning,
or loss of required domain expressiveness; do not silently convert an application
predicate into a guarantee. Required real-domain examples and acceptable losses
are not yet supplied, so sufficiency of this envelope remains unestablished.

## 3. Evidenced exclusions versus prototype rejection

**Settled boundaries:** not any possible type (HUMAN `.91`); no cyclic descriptor
closure or programming-language parsing; no automatic inference of arbitrary
application predicates (S:1193–1213); fixed producer classes (S:355–366);
no computed schema compatibility (S:297–303); no backend nouns in the public
vocabulary (S:591–595). The map's standing surface discipline retains HUMAN
`jn1.8 D6`: no public serde on Pardosa types at 1.0.

**Prototype rejections, not newly established standard exclusions:** maps/sets,
raw collection/text spellings, platform-sized integers, pointers/references and
selected serde attributes. In particular, map/set rejection diagnoses
**gh-report doctrine** (`G/pardosa-derive/src/reject.rs:131–147`, corrected
orientation). That is not a Pardosa HUMAN refusal. This distinction admits none
of these types: an undefined constructor cannot be used merely because it has
not been permanently refused.

Corrections carried forward explicitly: `EventF` includes NaN/infinities, is not
the no-NaN wrapper, and is not total over all IEEE inputs; checked `Real`/`Ordered`
do not admit all finite floats. `NotReal` is an error-code name, not float
membership (S:993). C8.3 assigns schema mapping/upcasting/admissibility to the
application, not all value-domain decisions (S:1443–1448). It does not categorically
ban declarative refinement or all runtime validation. No historical Rust
representation spelling is promoted into strategic membership here.

## 4. One HUMAN decision — four adopted definitions

The **Definition follow-up** (`bd show pardosa-ha3s`) is the single HUMAN decision
owner. The **Historical definition proposal** (`bd show pardosa-3nn1`, under
`pardosa-32qn`) preserves the original proposal and explicit postponement.
The new HUMAN comment on ha3s selects adoption; the earlier postponement is
historical, not erased or retroactively reinterpreted. Source-tested orientation
`pardosa-6vof` narrows the earlier individual-versus-pair ambiguity; oracle
`pardosa-p285` supplies constraints, with its stale references and categorical
identity/containment claims corrected by that orientation and current sources.

| Noun | Accepted definition in sentence form | Existing carrier and boundary |
| --- | --- | --- |
| artefact | An artefact is a unit of typed-record history managed by pardosa through one dragline. | C5.60; S:36–42,829–830,1462–1465. Individual history is now the explicit noun boundary by new HUMAN authority, not a recovered historical glossary ruling. One dragline and whole-history commitment remain. |
| epoch | An epoch identifies one term in the succession of ownership of an artefact. | C5.7; S:410–428. No width, clock duration, increment mechanism, global uniqueness or public epoch API. Ownership succession is distinct from migration generation. |
| anchor | An anchor is evidence of an artefact's rolling commitment at an observation, held by an external observer. | C5.26; S:559–566,585–589,936–938. Not whole state or a required timestamp/receipt. Election, unanchored-not-invalid and per-generation scope remain stated once in their existing clauses. |
| locator | A locator is an external name for an artefact's dragline. | C6.5 replaces its existing naming sentence rather than duplicating it; S:266–270,795–797,936–945. Distinct from logical dataset identity; no path/URL/key format, uniqueness, stability or consumer-held identity. |

### Source model and counterexample

Direct owning comments establish the boundary: ownership **R12–13 and R16**
(`pardosa-jn1.30`) make the ownership record a managed store and terminate its
self-claim recursion; **R15** specifies naming/pairing, not a universal dictionary
definition. **R45, R47–49** (`pardosa-jn1.42`) retain self-governance, one record
per data-store artefact, ordinary chained format, and co-movement without
atomicity. **R97–99** (`pardosa-jn1.53`, read with its citation erratum) promise
one dragline and whole-artefact commitment scope. These sources support individual
managed histories, not a new choice between two integrity architectures.

Take event history `E = [e1, e2]` and ownership history
`O = [claim(a, epoch1), release(a), claim(b, epoch2)]`. Under C10.1 and C5.60,
`cE` covers E and `cO` covers O. An additional ownership record changes O without
becoming an event in E. O carries its own claim (C5.8); no O-of-O is required.
This is a documentary scenario, not an executed protocol test: it specifies no
transaction schedule, separate extra epoch, writer relationship or public cO API.

A composite `P = (E, O)` with a self-governed O passes the recursion test but
does not gain a whole-pair commitment: cE excludes O's history, cE plus cO is not
one commitment, and inventing cPair adds unruled coverage. Calling the operational
pair a collective leaves contextual language to reconcile, not a competing
integrity guarantee. Ordered creation alone does not distinguish those usages.
Shared format, shared naming and co-movement (S:108–112,445–456,1476–1479) imply
neither one physical container nor universal nesting/non-nesting.

**C6.6 provenance:** commit `4341029a` introduced “the dragline that artefact is”
while compressing the roster wording; the directly read **R186** in
`pardosa-jn1.61` rules own identification/no roster, not noun synonymy. R184–189
are recorded as AUTONOMOUS, not individual HUMAN noun rulings. One-to-one scope
alone does not prove two concepts synonymous. This is editorial provenance for
minimal reconciliation; authority for the present edit is the new ha3s HUMAN
comment, not that editorial history. C6.6 now identifies the artefact's dragline.

Deleting only “no artefact stands within another” does not repair the old
proposal: **outermost** still selects a hierarchy, and **every establishment**
overreaches the scoped rolling-commitment/order claims. Migration disagreement
and dataset membership explicitly relate records (S:487–492,1344–1346).
The accepted terminology supplies no permission to invent pair-wide integrity.
A new Stance clause would require separate authority beyond
partition-only RC-1. The property/mechanism rule is R153 (current C5.53), not
the old proposal's stale C5.55 reference. Positive drafts and existing carriers
avoid a second glossary; operational consequences remain with their owners.

### Exact HUMAN acceptance and bounded delivery

HUMAN selected **Adopt definitions (Recommended)** on ha3s, 2026-09-07 09:30:

> Use artefact for an individual managed history and adopt epoch, anchor and locator as defined above.

The exact presented noun definitions are retained in that HUMAN comment. The
table renders them as sentences using the specification's lowercase product name.
C5.60/C5.7/C5.26/C6.5 carry the four properties; C6.6 removes editorial synonymy,
C4.14 removes the locator's physical qualifier, and C5.10 describes incomplete
creation of the event-data artefact when its ownership history already exists.
C5.11 still binds the two by shared name without content inspection; C3.5 and
C10.3 preserve co-movement, shared format and filesystem naming without atomicity.
These association clauses need no new container law or duplicated definition.
The original “outermost”, whole-state and locator-decline proposal is superseded,
not adopted. Its historical body and HUMAN postponement comments remain intact.

## 5. Current readiness — separate three checkpoints

| Checkpoint | Actual status and retained obligations |
| --- | --- |
| Decision-level planning / map closure | **Decision-complete; READY for 0.5.1 implementation planning.** HUMAN has disposed both the inventory planning gate and the noun adoption question. Independent semantic/live-use review APPROVE is recorded in `pardosa-aovc`; commander `pardosa-gudk.2` determines the map destination met. No further HUMAN semantic choice was identified in the bounded open-work assessment. S3 retains semantic completion and the full transfer register stays intact. Administrative closure status is recorded in bd (`pardosa-gudk.2` and `pardosa-jn1`). |
| Publication starting at 0.5.1 | **Not established.** Every used constructor needs its complete defined meaning and supported-route preservation; portable structural completeness and applicable conformance remain owed (S:1193–1219,1427–1439). No knowingly unmet binding obligation ships (S:815–818). Discovery can precede completeness; publication cannot. |
| 1.0 freeze | **Not established.** Final semantic inventory, widths, actual shipped producer names and retained implementation/release obligations remain owed. Noun adoption does not complete those duties. Surface evolution during 0.5.x is not permission to publish undefined semantics (S:161–178,355–366). |

Protected rulings are unchanged: dragline-local valid-by-construction cursors
(C5.22); application-owned mapping/upcasting/admissibility (C8.3); source retirement
and established write authority (C5.63); qualified partial reads and independent
knowledge, including genuine unknowns (C6.7–8, C6.15–17); ordinary schema/integrity
checks (C5.27–28, C5.51–52); all producer and release commitments above.
The completed answer/admission census is not an unfinished generic audit and is
not a completed descriptor inventory. `.92/.93` remain bounded NO-WITNESS
findings, not policy votes. Optional editorial partition is not a readiness gate.
No downstream code, conformance, MSRV or public-surface-gate delivery is claimed.

## 6. Selected HUMAN disposition and downstream owner

HUMAN selected **Bounded discovery (Recommended)** on 2026-09-07. Exact acceptance:

> Complete the catalogue during 0.5.1 development; define every supported type before publication and escalate material semantic changes.

The inventory planning gate is disposed, not the catalogue implemented. The
existing future transfer-map planning register in `pardosa-jn1`, **S3 Descriptor
semantic completion and realization**, owns completion during 0.5.1 development:
constructor membership, value domains, widths, bounds and units, emptiness,
parameter admissibility, and preservation of assigned constraints through every
supported construction and decoding route. This is semantic work, not merely
encoding. Complete specification definitions precede publication of each pardosa
release, starting at 0.5.1. Material new family or constraint meanings, or loss of
required domain expressiveness, return for HUMAN disposition. The curated policy,
closed sets, producer restrictions and C4.1 release regime remain unchanged.
Implementation observations may inform definitions; they never substitute for
normative specification text. No new transfer map is created and all existing
20 base and 10 supplementary obligations remain carried.

## 7. Bounded closure-readiness audit

Read-level audit of the current map's T1–T20/S1–S10 carrier index and .89, plus
the actual review bodies, not a fresh whole-corpus source-fidelity certification:

| Coverage | Existing carrier / surviving work |
| --- | --- |
| T1–T4 | Format/wire, unknown-tag and multi-region format tension, clean-room transfer/migration/wall, stronger derive seal; mixed normative outcomes remain in the spec. |
| T5–T8 | File exclusion predicates and both-backend conformance; timestamp planning without a sixth envelope field; F2 explicitly LAPSED, no continuing prohibition. |
| T9–T12 | Published-library missing-docs enforcement, strict create/open and legacy rescue, documentation citation process without new gate, ordering/chaining conformance. |
| T13–T16 | Positive fence/walk/validation promises and explicit rescue election; redesign/block_on construction; publishing and production-based promotion judgement. |
| T17–T20 | Test scaffolding with unstable-test-support exception, gh-report adoption, immutable foreign PGNs, and reader CLI deferral remain outside; read capability/migration tool are not deferred. |
| S1–S3 | Qualified-result representation; established authority/prior-landing proof and recovery; descriptor semantic completion under the HUMAN-disposed .91 planning gate. S3 is not mere encoding. |
| S4–S7 | Derive cycle compile-fail fixture (not facade suite), truthful seal documentation, dependency-graph MSRV proof, identifier rustdoc obligations. |
| S8–S10 | Application-owned transforms; public-surface and semver gates with distinct timing; actual shipped producer names, not prototype names. |

All 20 base and ten supplementary carriers remain in the existing map; none is a
completed downstream deliverable by virtue of being indexed. Their full text is
retained there rather than replaced by this grouped audit. S3 still requires exact
constructor meanings and supported-route preservation before every publication
from 0.5.1; bounded discovery disposes .91's planning gate, not that work.

**Review accounting:** `pardosa-w15k` approves the earlier eight-clause
source/trace reconciliation, explicitly not a fresh exhaustive 244-ruling audit.
`pardosa-pssq` initially approved the answer/admission source diff while rejecting
decision preparation; its final scoped re-review resolves M1/L1/L2 and approves
that delivery. Its 162-clause census is bounded answer/admission evidence, not
complete descriptor semantics or proof of zero unknown defects. `pardosa-t5di`
approves the later bounded-discovery disposition and S3 expansion. These are
read, attributed review records, not fresh execution of those historical reviews.
The earlier four-definition preparation received independent documentary
APPROVE in `pardosa-j8i7`, not approval of the current normative diff or map.

**Closure predicate satisfied at decision level:** explicit HUMAN noun disposition,
source-faithful delivery independently APPROVED in `pardosa-aovc`, and the complete
retained transfer register support the commander READY verdict in `pardosa-gudk.2`.
Gardener records administrative closure in bd. Current approval is not inherited
from a historical review; no additional semantic hold is created here.
Publication and 1.0 freeze remain the separate checkpoints in section 5.

**Historical open-work snapshot (2026-09-07, before gardener closure):**
`bd list --status open --limit 0`
returned 65 records before this mission was claimed; the in-progress query was
empty. The map's only open child was .89. Its 25-unit disposition index identified
ha3s/3nn1 as the last semantic planning hold, already HUMAN-disposed. At finalization,
ha3s, historical 3nn1/32qn, .89 and jn1 were open only for administrative closure, not
another noun ballot. ukwd is optional content-preserving partition, not a planning
gate. Open historical evidence, tooling-review and fleet/tooling records are not
new product choices merely because their status is open. This bounded inventory
does not certify every historical evidence bead or an exhaustive absence of defects.
The destination remains a written specification before chartering the separate
clean-room transfer map; neither implementation planning nor publication happened.

**Live-use preservation matrix (read-level, not protocol execution):**

| Surface exercised | Result and limit |
| --- | --- |
| E, O, cE, cO | C5.60 and C10.1 cover each history separately; O self-governs under C5.8. An O-only append changes cO, not the E event line; no cPair, O-of-O, extra epoch or public commitment API follows. |
| Ownership and interrupted creation | C5.7 retains CAS win/loss and stale-write rejection. C5.9 retains unowned empty O. C5.10 retains O-first creation, open/completion, write refusal for missing O, qualified reads and no repair; C5.62 retains strict create/open. |
| Anchor observation and migration | C5.26 adds the noun but retains elected anchoring, unanchored-not-invalid and full-rewrite non-resistance; C5.29 retains generation-local scope and re-anchoring. |
| Locator, dataset and operator | C4.14/C5.55 keep logical dataset identity distinct from locator; C6.5/6 keep external naming and operator reporting without consumer-held identity; C6.37 still has no roster. |
| Protected HUMAN facts | Cursor C5.22; resubmission C5.12; cutover C5.63; partial/independent knowledge C6.7–8/15–17; application transforms C8.3; citation authoring note; curated descriptor/derive C4.24/C6.23; ordinary validation, producer limits and C4.1 regimes remain unchanged. |

The independent current-diff review and commander readiness determination are
complete (`pardosa-aovc`, `pardosa-gudk.2`). No substantive planning deficit was
demonstrated. Catalogue completion, runtime proof and optional editorial work are
not claimed completed; their existing dispositions and downstream owners survive.

## Verification scope

This mission edits the specification, supplementary trace notes, this report and
existing decision navigation in bd. No clause identifier, regime, numeric ruling
identity or classification input changes. Inner
verification is `git diff --check`; mid verification is `./scripts/check.sh`;
boundary status is `git status --short`. The local gate checks spec/trace
consistency, not this report's semantic judgement or implementation readiness.
Exact exits, preserved navigation preimages and review handoff evidence belong
to the adoption mission's durable evidence bead. No E2E implementation verify is
specified; documentary checks only. Independent review APPROVE and commander
READY are recorded in `pardosa-aovc` and `pardosa-gudk.2`. The authorized commit
contains only the three intended documents; its identity and terminal verification
are recorded on `pardosa-gudk.2`. Gardener owns ticket closures. No push or sync.
