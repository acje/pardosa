# Semantic reconciliation — decision preparation, not a specification

Status: **NONNORMATIVE; scoped documentary approval in `pardosa-7vrc`.**
Review corrections applied; approval establishes neither semantic completeness
nor implementation, map or release readiness.
Prepared 2026-09-07 under `pardosa-higv` / `pardosa-higv.1`, against spec
HEAD `71d6a5e`. No constructor, definition, API or HUMAN option is adopted here.
The [specification](../pardosa-1.0.md) alone carries normative commitments
(C5.2). This report is planning evidence containing prototype observations; it
is not a clean-room implementation input and does not cross that wall.

## Evidence and reading rule

`S` means `docs/spec/pardosa-1.0.md`; `T` means
`docs/spec/trace/ruled-trace.tsv`. Line references below describe this baseline.
Both were inspected directly. Prototype references `G/...` mean
`../gh-report/crates/...`, as source-checked in **Corrected semantic orientation**
(`bd show pardosa-nsru`), read before earlier evidence. They are attributed
source observations, not fresh prototype reads or executed tests in this mission.
Earlier **Inventory observation** (`bd show pardosa-sdrh`) contributes candidate
details only subject to that correction register; **Oracle summary**
(`bd show pardosa-oqlf`) is not an independent correctness certificate.

Current HUMAN `.91` (2026-09-06 22:01) selects a curated constrained vocabulary
and derive guidance, with any-possible-type support an explicit non-goal.
It selects neither a float ballot nor the prototype catalogue. Current
**Descriptor semantics** (`bd show pardosa-jn1.91`) and **Remaining holds**
(`bd show pardosa-jn1.89`) preserve the unfinished semantic inventory.

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

Proposed envelope: finish a small set of domain-useful scalar meanings, bounded
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

## 4. Four nouns — recovered meaning, not adopted definitions

The **Definition follow-up** (`bd show pardosa-ha3s`) and **Definition proposal**
(`bd show pardosa-3nn1`) remain HUMAN-postponed. Documentary investigation is
authorized now; noun rulings are not. The old proposal is evidence to revise,
not four pre-approved sentences awaiting insertion.

| Noun | Source-backed explanatory recovery | Outstanding limit |
| --- | --- | --- |
| epoch | Names a term of ownership; later ownership supersedes earlier ownership for write admission. The ownership record supplies it. | S:410–428,1101–1103,1392–1399. Do not add a width, increment algorithm or public epoch API. Distinct from a migration generation. |
| anchor | Evidence associated with the rolling commitment, held by an external observer, supporting the elected rewrite-detection claim for one artefact in one generation. | S:559–566,585–589,936–938. “Whole artefact state at one moment” overstates the recovered meaning. No proof carrier, publication or addressing API is selected. Unanchored remains distinct from invalid. |
| locator | Names the dragline from outside, distinct from the operator's logical dataset identity. | S:266–270,936–945,795–797. No path/URL/key format, uniqueness law or consumer identity is implied. **Do-not-define was a recommendation, never adopted.** Formal disposition remains HUMAN-owned. |
| artefact | One dragline and its rolling commitment are artefact-scoped; an ownership record is itself an artefact with a typed record line. | S:829–830,1461–1478. Individual managed line versus paired managed unit remains unresolved; this row is not a replacement definition. |

The precise artefact tension is between “holds” (C5.60), “is” (C6.6),
ownership-record artefact status (C10.1), and paired creation/naming/movement
(C5.10–11, C3.5). “Nothing governs that record from above” terminates ownership
recursion (S:432–435); it does not establish spatial containment. “One container
format” (S:1475–1478) is not “one physical container”. Neither universal nesting
nor universal non-nesting follows.

Deleting only “no artefact stands within another” does not repair the old
proposal: **outermost** still selects a hierarchy, and **every establishment**
overreaches the scoped rolling-commitment/order claims. Migration disagreement
and dataset membership explicitly relate records (S:487–492,1344–1346).
Source recovery therefore leaves this boundary unresolved. A new Stance clause
would additionally need authority beyond partition-only RC-1. No spec wording,
clause location or locator disposition is selected here.

## 5. Current readiness — separate three checkpoints

| Checkpoint | Actual status and retained obligations |
| --- | --- |
| Map closure | **Not established.** Curated boundary is settled; remaining constructor membership/meanings/widths need resolution or explicit HUMAN disposition. The noun set remains postponed, not waived. Source-backed coverage and independent review are required; the existing transfer register remains intact. A strategy choice alone closes neither definitions nor map. |
| Publication starting at 0.5.1 | **Not established.** Every used constructor needs its complete defined meaning and supported-route preservation; portable structural completeness and applicable conformance remain owed (S:1193–1219,1427–1439). No knowingly unmet binding obligation ships (S:815–818). Discovery can precede completeness; publication cannot. |
| 1.0 freeze | **Not established.** Final semantic inventory, widths, all postponed noun dispositions, actual shipped producer names and retained implementation/release obligations remain owed. Surface evolution during 0.5.x is not permission to publish undefined semantics (S:161–178,355–366). |

Protected rulings are unchanged: dragline-local valid-by-construction cursors
(C5.22); application-owned mapping/upcasting/admissibility (C8.3); source retirement
and established write authority (C5.63); qualified partial reads and independent
knowledge, including genuine unknowns (C6.7–8, C6.15–17); ordinary schema/integrity
checks (C5.27–28, C5.51–52); all producer and release commitments above.
The completed answer/admission census is not an unfinished generic audit and is
not a completed descriptor inventory. `.92/.93` remain bounded NO-WITNESS
findings, not policy votes. Optional editorial partition is not a readiness gate.
No downstream code, conformance, MSRV or public-surface-gate delivery is claimed.

## 6. One HUMAN strategy question — both options unselected

**Where should authority to finish the remaining finite semantic baseline reside,
and when should that baseline be disposed?**

| Option | Authority and timing | Tradeoff |
| --- | --- | --- |
| **A — Recommended, UNSELECTED: bounded implementation discovery** | HUMAN explicitly disposes finite baseline completion to 0.5.1 development within the settled curated boundary. Exact semantic definitions precede each release. Material new family/constraint meanings or loss of required domain expressiveness return for HUMAN disposition. | Enables discovery without width-by-width or float-by-float ballots; requires disciplined semantic completion and escalation. Does not adopt this catalogue or authorize undefined release constructors. |
| **B — UNSELECTED: semantic inventory first** | Retain the residual `.91` membership/meaning inventory as an upfront strategic gate; prepare and dispose one coherent catalogue before implementation claims that baseline. | More upfront confidence in the intended domain envelope, at the cost of delaying implementation discovery. Still not a sequence of unrelated scalar polls. |

These are mutually exclusive locations/timings of completion authority, not
competing float types. Neither option decides the four nouns, closes their
HUMAN holds, creates the transfer map, or weakens any release obligation.

## Verification scope

This mission changes no normative spec text, RULED trace row or Meadows
classification. The existing local `./scripts/check.sh` checks the unchanged
spec/trace contract; it cannot certify this report's semantic judgement or
implementation readiness. Meadows reclassification verification is not applicable
without classification changes. Exact command exits and bead-navigation update
evidence belong in `pardosa-higv.1`; scoped independent commander approval is
recorded in `pardosa-7vrc`, with its citation and status corrections applied.
