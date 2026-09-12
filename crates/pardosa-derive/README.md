# pardosa-derive

Procedural macro deriving compile-time `PardosaSchema` AST descriptors and binary wire codecs for [Pardosa](https://crates.io/crates/pardosa).

## Overview

`pardosa-derive` provides the `#[derive(PardosaSchema)]` procedural macro to generate:
- Declared schema versions via `#[pardosa(version = N)]` (defaults to `1`).
- Strongly-typed AST schema descriptors capturing enum variants and typed fields.
- Canonical 32-byte BLAKE3 schema identity fingerprints (`SchemaIdentity`).
- Deterministic binary wire encoding (`encode_payload`) and decoding (`decode_payload`) routines adhering to Pardosa format rules.

## Macro Usage & Attributes

The derive macro is applied to an `enum` representing domain events.

```rust
use pardosa::encoding::EventString;
use pardosa::schema::PardosaSchema;

#[derive(Debug, Clone, PartialEq, Eq, PardosaSchema)]
#[pardosa(version = 1)]
pub enum OrderEvent {
    #[pardosa(tombstone)]
    Tombstone = 0,

    Created {
        order_id: EventString<64>,
        amount_cents: u64,
    } = 1,

    Shipped {
        tracking_number: EventString<64>,
    } = 2,

    Cancelled = 3,
}
```

### Attributes

- `#[pardosa(version = N)]`: Container-level attribute declaring the schema version (positive integer).
- `#[pardosa(tombstone)]`: Variant-level attribute designating the migration tombstone variant. Exactly one variant must be designated.
- Explicit integer discriminants: Every variant must declare an explicit integer discriminant (`Variant = 0`).
  - Discriminants `<= 255` use 1-byte wire representation.
  - Discriminants `> 255` use 2-byte little-endian wire representation.

## Compile-Time Diagnostics & Shape Rules

Input types are validated at compile time, rejecting unsupported shapes with actionable diagnostics:
- **Enum Root Only**: Structs and unions are rejected as payload roots per C5.48 and C4.24. Payload data must be wrapped in an enum.
- **Explicit Discriminants Required**: Every variant must have an explicit discriminant.
- **Tombstone Required**: Exactly one variant must be marked with `#[pardosa(tombstone)]`.
- **Unbounded Types Rejected**: Raw `String`, unbounded `Vec`, and floating-point types (`f32`, `f64`) are excluded per C6.23. Use bounded alternatives such as `EventString<MAX>`, `NonEmptyEventString<MAX>`, `EventVec<T, MAX>`, `EventBytes<MAX>`, or fixed-width / scaled integers.
- **Cyclic Types Detected**: Recursive types without indirection are detected and rejected per C6.22 (S4).

## Truthful Seal Limits

Per C4.24 and C6.35, `PardosaSchema` derive guarantees that generated descriptors accurately reflect the compiled Rust type definition. Shipped foundation crates contain zero hand-written descriptor implementations. Runtime semantic fidelity of user domain fields remains outside what Pardosa establishes.

## Security & Maintenance

- **Security Reporting**: Report vulnerabilities privately via GitHub Security Advisories at [https://github.com/acje/pardosa/security/advisories](https://github.com/acje/pardosa/security/advisories) or contact `security@pardosa.dev`.
- **Withdrawal Posture**: Published releases are yanked strictly for correctness or safety defects.

## License

Licensed under either of [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) or [MIT License](https://opensource.org/licenses/MIT) at your option.
