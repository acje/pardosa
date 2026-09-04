//! Mechanically verify `scripts/data/ruled-meadows.tsv` against the house Meadows
//! layer->tier mapping and the RULED ownership spine in `scripts/data/ruled-owners.tsv`.
//!
//! Usage: cargo run --bin meadows-check -- [--meadows <tsv>] [--owners <tsv>]
//! Output: tab-separated ROW / LAYER / TIER / SUMMARY records, plus FAIL records on
//! any violation. Exits 0 when every check passes, 1 otherwise.

use std::collections::BTreeMap;
use std::fmt;

const EXPECTED_LO: u32 = 1;
const EXPECTED_HI: u32 = 244;
const FIELD_COUNT: usize = 5;
const DOMINANCE_THRESHOLD: f64 = 80.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Layer(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    S,
    A,
    B,
    C,
    D,
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Tier::S => "S",
            Tier::A => "A",
            Tier::B => "B",
            Tier::C => "C",
            Tier::D => "D",
        };
        f.write_str(s)
    }
}

impl Tier {
    fn parse(s: &str) -> Option<Tier> {
        match s {
            "S" => Some(Tier::S),
            "A" => Some(Tier::A),
            "B" => Some(Tier::B),
            "C" => Some(Tier::C),
            "D" => Some(Tier::D),
            _ => None,
        }
    }
}

impl Layer {
    fn parse(s: &str) -> Option<Layer> {
        match s.parse::<u8>() {
            Ok(n) if (1..=12).contains(&n) => Some(Layer(n)),
            _ => None,
        }
    }

    fn tier(self) -> Tier {
        match self.0 {
            1..=3 => Tier::S,
            4 => Tier::A,
            5 | 6 => Tier::B,
            7 | 8 => Tier::C,
            _ => Tier::D,
        }
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug)]
struct Row {
    ruled_n: u32,
    layer: Layer,
    tier: Tier,
    owning_ticket: String,
    justification: String,
}

#[derive(Debug)]
struct OwnerRange {
    ticket: String,
    lo: u32,
    hi: u32,
}

fn arg_or(args: &[String], flag: &str, default: &str) -> String {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].clone())
        .unwrap_or_else(|| default.to_string())
}

fn load_owners(path: &str) -> Vec<OwnerRange> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read owners tsv {path}: {e}"));
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            assert_eq!(f.len(), 3, "owners row must have 3 fields: {l}");
            OwnerRange {
                ticket: f[0].to_string(),
                lo: f[1].parse().expect("owners lo must be a number"),
                hi: f[2].parse().expect("owners hi must be a number"),
            }
        })
        .collect()
}

fn owner_for(owners: &[OwnerRange], n: u32) -> Option<&str> {
    owners
        .iter()
        .find(|r| n >= r.lo && n <= r.hi)
        .map(|r| r.ticket.as_str())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let meadows_path = arg_or(&args, "--meadows", "scripts/data/ruled-meadows.tsv");
    let owners_path = arg_or(&args, "--owners", "scripts/data/ruled-owners.tsv");

    let owners = load_owners(&owners_path);
    let text = std::fs::read_to_string(&meadows_path)
        .unwrap_or_else(|e| panic!("cannot read meadows tsv {meadows_path}: {e}"));

    let mut failures: Vec<String> = Vec::new();
    let mut rows: Vec<Row> = Vec::new();

    for (idx, line) in text.lines().enumerate() {
        let lineno = idx + 1;
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != FIELD_COUNT {
            failures.push(format!(
                "FAIL\tFIELDCOUNT\tline {lineno}\texpected {FIELD_COUNT} fields, found {}",
                fields.len()
            ));
            continue;
        }
        if let Some(empty) = fields.iter().position(|f| f.trim().is_empty()) {
            failures.push(format!(
                "FAIL\tEMPTYFIELD\tline {lineno}\tcolumn {}",
                empty + 1
            ));
            continue;
        }
        let ruled_n: u32 = match fields[0].parse() {
            Ok(n) => n,
            Err(_) => {
                failures.push(format!(
                    "FAIL\tRULEDN\tline {lineno}\tnot a number: {}",
                    fields[0]
                ));
                continue;
            }
        };
        let layer = match Layer::parse(fields[1]) {
            Some(l) => l,
            None => {
                failures.push(format!(
                    "FAIL\tLAYER\truled {ruled_n}\tnot in 1..=12: {}",
                    fields[1]
                ));
                continue;
            }
        };
        let tier = match Tier::parse(fields[2]) {
            Some(t) => t,
            None => {
                failures.push(format!(
                    "FAIL\tTIER\truled {ruled_n}\tnot S|A|B|C|D: {}",
                    fields[2]
                ));
                continue;
            }
        };
        if tier != layer.tier() {
            failures.push(format!(
                "FAIL\tTIERMISMATCH\truled {ruled_n}\tlayer {layer} derives {} but row says {tier}",
                layer.tier()
            ));
        }
        match owner_for(&owners, ruled_n) {
            Some(expected) if expected == fields[3] => {}
            Some(expected) => failures.push(format!(
                "FAIL\tOWNER\truled {ruled_n}\tspine says {expected} but row says {}",
                fields[3]
            )),
            None => failures.push(format!(
                "FAIL\tOWNERMISSING\truled {ruled_n}\tno owning range in spine"
            )),
        }
        if fields[4].contains('\t') {
            failures.push(format!(
                "FAIL\tJUSTIFICATION\truled {ruled_n}\tcontains a tab"
            ));
        }
        rows.push(Row {
            ruled_n,
            layer,
            tier,
            owning_ticket: fields[3].to_string(),
            justification: fields[4].to_string(),
        });
    }

    let mut seen: BTreeMap<u32, usize> = BTreeMap::new();
    for r in &rows {
        *seen.entry(r.ruled_n).or_insert(0) += 1;
    }
    for (n, count) in &seen {
        if *count > 1 {
            failures.push(format!("FAIL\tDUPLICATE\truled {n}\tappears {count} times"));
        }
    }
    for n in EXPECTED_LO..=EXPECTED_HI {
        if !seen.contains_key(&n) {
            failures.push(format!("FAIL\tGAP\truled {n}\tmissing"));
        }
    }
    for n in seen.keys() {
        if *n < EXPECTED_LO || *n > EXPECTED_HI {
            failures.push(format!(
                "FAIL\tOUTOFRANGE\truled {n}\toutside {EXPECTED_LO}..={EXPECTED_HI}"
            ));
        }
    }

    let mut by_layer: BTreeMap<Layer, usize> = BTreeMap::new();
    let mut by_tier: BTreeMap<Tier, usize> = BTreeMap::new();
    for r in &rows {
        *by_layer.entry(r.layer).or_insert(0) += 1;
        *by_tier.entry(r.tier).or_insert(0) += 1;
    }

    let total = rows.len();
    println!("ROW\ttotal\t{total}");
    println!("ROW\texpected\t{}", EXPECTED_HI - EXPECTED_LO + 1);
    for (layer, count) in &by_layer {
        let pct = 100.0 * *count as f64 / total.max(1) as f64;
        println!("LAYER\t{layer}\t{}\t{count}\t{pct:.1}%", layer.tier());
    }
    for (tier, count) in &by_tier {
        let pct = 100.0 * *count as f64 / total.max(1) as f64;
        println!("TIER\t{tier}\t{count}\t{pct:.1}%");
    }
    let mut tickets: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &rows {
        *tickets.entry(r.owning_ticket.as_str()).or_insert(0) += 1;
    }
    println!("SUMMARY\towning_tickets\t{}", tickets.len());
    let shortest = rows
        .iter()
        .map(|r| r.justification.chars().count())
        .min()
        .unwrap_or(0);
    println!("SUMMARY\tshortest_justification_chars\t{shortest}");

    if let Some((layer, count)) = by_layer.iter().max_by_key(|(_, c)| **c) {
        let pct = 100.0 * *count as f64 / total.max(1) as f64;
        println!("SUMMARY\tdominant_layer\t{layer}\t{count}\t{pct:.1}%");
        if pct > DOMINANCE_THRESHOLD {
            failures.push(format!(
                "FAIL\tDOMINANCE\tlayer {layer}\tholds {pct:.1}% of rows, above {DOMINANCE_THRESHOLD:.0}%"
            ));
        }
    }

    if failures.is_empty() {
        println!("SUMMARY\tverdict\tPASS");
        std::process::exit(0);
    }
    for f in &failures {
        println!("{f}");
    }
    println!("SUMMARY\tverdict\tFAIL\t{}", failures.len());
    std::process::exit(1);
}
