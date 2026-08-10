# pardosa

Event-sourcing storage substrate for agent-first EDA development.

Currently developed in-tree at [Mattilsynet/gh-report](https://github.com/Mattilsynet/gh-report)
under `crates/pardosa*`. This repo is the destination for the extracted,
standalone library.

## Status

Charting. This repo holds a [wayfinder](https://github.com/mattpocock/skills)
map — a bd epic whose child tickets are the open decisions between here and a
defined end state for the library.

Run `bd show <map-id>` to read the map; `bd ready --parent <map-id> -u` for the
frontier.

## Crates (as they stand in gh-report)

| Ring       | Crate                  | Publish | Purpose                                    |
|------------|------------------------|---------|--------------------------------------------|
| substrate  | `pardosa-wire`         | yes     | `no_std` canonical encode/decode           |
| substrate  | `pardosa-derive`       | yes     | proc-macros (`GenomeSafe`, schema derives) |
| substrate  | `pardosa-file`         | yes     | `.pgno` container writer/reader            |
| substrate  | `pardosa-nats`         | no      | JetStream backend                          |
| vocabulary | `pardosa-schema`       | yes     | typed payload vocabulary                   |
| runtime    | `pardosa`              | yes     | `EventStore` facade — the public surface    |
| adapter    | `pardosa-fiber-store`  | no      | sync one-key-one-fiber adapter             |
| adapter    | `pardosa-read`         | no      | read-only CLI                              |

Ring dependencies are one-directional (PGN-0001). External consumers depend
only on `pardosa`.

## License

Dual-licensed under Apache-2.0 and MIT, matching gh-report.
