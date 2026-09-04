//! Mechanically check coverage + shape of `docs/spec/pardosa-1.0.md` against the
//! trace table `docs/spec/trace/ruled-trace.tsv` and the expected ruled_n set in
//! `scripts/data/ruled-meadows.tsv`.
//!
//! Usage: cargo run --bin spec-coverage -- [--spec <md>] [--trace <tsv>]
//!        [--meadows <tsv>] [--range <lo>..<hi>] [--allow-regime-prose <clause-id>]...
//! Output: tab-separated CHECK / SUMMARY records. Exits 0 on overall PASS, 1 on FAIL.
//! Fail-open guard: an absent or empty spec document, or an empty trace table,
//! MUST produce verdict FAIL.

use std::collections::{BTreeMap, BTreeSet};

const LAYER_ORDER: [u32; 9] = [2, 3, 4, 5, 6, 8, 9, 10, 12];
const STATUS_OPEN: &str = "<!-- STATUS -->";
const STATUS_CLOSE: &str = "<!-- /STATUS -->";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Regime {
    Invariant,
    Surface,
}

impl Regime {
    fn parse(s: &str) -> Option<Regime> {
        match s {
            "INVARIANT" => Some(Regime::Invariant),
            "SURFACE" => Some(Regime::Surface),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct Clause {
    id: String,
    layer: u32,
    n: u32,
    regime: Regime,
    body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    SpecBearing,
    MapGovernance,
}

impl Disposition {
    fn parse(s: &str) -> Option<Disposition> {
        match s {
            "SPEC-BEARING" => Some(Disposition::SpecBearing),
            "MAP-GOVERNANCE" => Some(Disposition::MapGovernance),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct TraceRow {
    ruled_n: u32,
    disposition: Disposition,
    clause_ids: Vec<String>,
    note: String,
}

struct CheckResult {
    name: &'static str,
    pass: bool,
    detail: String,
}

fn check(name: &'static str, pass: bool, detail: impl Into<String>) -> CheckResult {
    CheckResult {
        name,
        pass,
        detail: detail.into(),
    }
}

fn parse_clauses(doc: &str) -> Vec<Clause> {
    let heading_re =
        regex::Regex::new(r"^####\s+C(\d+)\.(\d+)\s+—\s+(INVARIANT|SURFACE)\s*$")
            .expect("clause heading regex must compile");
    let lines: Vec<&str> = doc.lines().collect();
    let mut clauses = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(caps) = heading_re.captures(line) {
            let layer: u32 = caps[1].parse().expect("layer digits must parse");
            let n: u32 = caps[2].parse().expect("clause n digits must parse");
            let regime = Regime::parse(&caps[3]).expect("regime already matched by regex");
            let id = format!("C{layer}.{n}");
            let mut body_lines = Vec::new();
            let mut j = i + 1;
            while j < lines.len() && !lines[j].trim_start().starts_with('#') {
                body_lines.push(lines[j]);
                j += 1;
            }
            clauses.push(Clause {
                id,
                layer,
                n,
                regime,
                body: body_lines.join("\n"),
            });
            i = j;
        } else {
            i += 1;
        }
    }
    clauses
}

fn find_status_block(doc: &str) -> Option<String> {
    let start = doc.find(STATUS_OPEN)?;
    let after_open = start + STATUS_OPEN.len();
    let end = doc[after_open..].find(STATUS_CLOSE)?;
    Some(doc[after_open..after_open + end].to_string())
}

fn parse_trace(text: &str) -> (Vec<TraceRow>, Vec<String>) {
    let mut rows = Vec::new();
    let mut malformed = Vec::new();
    let mut seen_header = false;
    for (idx, line) in text.lines().enumerate() {
        let lineno = idx + 1;
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if !seen_header {
            seen_header = true;
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 4 {
            malformed.push(format!("line {lineno}: expected 4 fields, found {}", fields.len()));
            continue;
        }
        let ruled_n: u32 = match fields[0].parse() {
            Ok(n) => n,
            Err(_) => {
                malformed.push(format!("line {lineno}: ruled_n not a number: {}", fields[0]));
                continue;
            }
        };
        let disposition = match Disposition::parse(fields[1]) {
            Some(d) => d,
            None => {
                malformed.push(format!("line {lineno}: unknown disposition {}", fields[1]));
                continue;
            }
        };
        let clause_ids: Vec<String> = if fields[2] == "-" {
            Vec::new()
        } else {
            fields[2].split(';').map(|s| s.to_string()).collect()
        };
        rows.push(TraceRow {
            ruled_n,
            disposition,
            clause_ids,
            note: fields[3].to_string(),
        });
    }
    (rows, malformed)
}

fn load_expected_ruled_ns(meadows_text: &str) -> BTreeSet<u32> {
    let mut set = BTreeSet::new();
    for line in meadows_text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 5 {
            continue;
        }
        if let Ok(n) = fields[0].parse::<u32>() {
            set.insert(n);
        }
    }
    set
}

fn check_trace_complete(
    expected: &BTreeSet<u32>,
    trace: &[TraceRow],
    range: (u32, u32),
) -> CheckResult {
    let expected_in_range: BTreeSet<u32> = expected
        .iter()
        .copied()
        .filter(|n| *n >= range.0 && *n <= range.1)
        .collect();
    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    for row in trace.iter().filter(|r| r.ruled_n >= range.0 && r.ruled_n <= range.1) {
        *counts.entry(row.ruled_n).or_insert(0) += 1;
    }
    let present: BTreeSet<u32> = counts.keys().copied().collect();
    let missing: Vec<u32> = expected_in_range.difference(&present).copied().collect();
    let duplicates: Vec<(u32, usize)> = counts
        .iter()
        .filter(|(_, c)| **c > 1)
        .map(|(n, c)| (*n, *c))
        .collect();
    let pass = missing.is_empty() && duplicates.is_empty();
    let mut detail = String::new();
    if !missing.is_empty() {
        let shown: Vec<String> = missing.iter().take(20).map(|n| n.to_string()).collect();
        detail.push_str(&format!(
            "missing {} (showing {}): {}; ",
            missing.len(),
            shown.len(),
            shown.join(",")
        ));
    }
    if !duplicates.is_empty() {
        let shown: Vec<String> = duplicates
            .iter()
            .take(20)
            .map(|(n, c)| format!("{n}x{c}"))
            .collect();
        detail.push_str(&format!(
            "duplicate {} (showing {}): {}",
            duplicates.len(),
            shown.len(),
            shown.join(",")
        ));
    }
    if pass {
        detail = format!("expected {} rows in range, all present exactly once", expected_in_range.len());
    }
    check("trace_complete", pass, detail)
}

fn check_spec_bearing_has_clause(trace: &[TraceRow], range: (u32, u32)) -> CheckResult {
    let offenders: Vec<u32> = trace
        .iter()
        .filter(|r| r.ruled_n >= range.0 && r.ruled_n <= range.1)
        .filter(|r| r.disposition == Disposition::SpecBearing && r.clause_ids.is_empty())
        .map(|r| r.ruled_n)
        .collect();
    let pass = offenders.is_empty();
    let detail = if pass {
        "all SPEC-BEARING rows in range carry >=1 clause id".to_string()
    } else {
        format!("SPEC-BEARING rows with no clause id: {offenders:?}")
    };
    check("spec_bearing_has_clause", pass, detail)
}

fn check_map_governance_shape(trace: &[TraceRow], range: (u32, u32)) -> CheckResult {
    let offenders: Vec<u32> = trace
        .iter()
        .filter(|r| r.ruled_n >= range.0 && r.ruled_n <= range.1)
        .filter(|r| {
            r.disposition == Disposition::MapGovernance
                && (!r.clause_ids.is_empty() || r.note.chars().count() < 20)
        })
        .map(|r| r.ruled_n)
        .collect();
    let pass = offenders.is_empty();
    let detail = if pass {
        "all MAP-GOVERNANCE rows in range have clause_ids=- and note>=20 chars".to_string()
    } else {
        format!("MAP-GOVERNANCE rows violating shape: {offenders:?}")
    };
    check("map_governance_shape", pass, detail)
}

fn check_clause_ids_resolve(trace: &[TraceRow], clauses: &[Clause]) -> CheckResult {
    let known: BTreeSet<&str> = clauses.iter().map(|c| c.id.as_str()).collect();
    let mut dangling: Vec<String> = Vec::new();
    for row in trace {
        for id in &row.clause_ids {
            if !known.contains(id.as_str()) {
                dangling.push(format!("{id} (ruled {})", row.ruled_n));
            }
        }
    }
    let pass = dangling.is_empty();
    let detail = if pass {
        "every cited clause id resolves to a spec clause heading".to_string()
    } else {
        format!("dangling clause ids: {}", dangling.join(", "))
    };
    check("clause_ids_resolve", pass, detail)
}

fn check_no_orphan_clause(trace: &[TraceRow], clauses: &[Clause]) -> CheckResult {
    let mut cited: BTreeSet<&str> = BTreeSet::new();
    for row in trace {
        if row.disposition == Disposition::SpecBearing {
            for id in &row.clause_ids {
                cited.insert(id.as_str());
            }
        }
    }
    let orphans: Vec<&str> = clauses
        .iter()
        .map(|c| c.id.as_str())
        .filter(|id| !cited.contains(id))
        .collect();
    let pass = orphans.is_empty();
    let detail = if pass {
        "every clause heading is cited by >=1 SPEC-BEARING row".to_string()
    } else {
        format!("orphan clauses: {}", orphans.join(", "))
    };
    check("no_orphan_clause", pass, detail)
}

fn check_regime_marker_unique(clauses: &[Clause], allow_regime_prose: &[String]) -> CheckResult {
    let token_re = regex::Regex::new(r"\b(INVARIANT|SURFACE)\b").expect("token regex must compile");
    let mut offenders: Vec<String> = Vec::new();
    for c in clauses {
        if allow_regime_prose.iter().any(|a| a == &c.id) {
            continue;
        }
        if token_re.is_match(&c.body) {
            offenders.push(c.id.clone());
        }
    }
    let pass = offenders.is_empty();
    let detail = if pass {
        "no clause body restates a regime marker".to_string()
    } else {
        format!("clause bodies with duplicate regime marker: {}", offenders.join(", "))
    };
    check("regime_marker_unique", pass, detail)
}

fn check_status_block(doc: &str, clauses: &[Clause]) -> CheckResult {
    let block = match find_status_block(doc) {
        Some(b) => b,
        None => return check("status_block", false, "STATUS block absent or malformed"),
    };
    let ref_re = regex::Regex::new(r"C\d+\.\d+").expect("status ref regex must compile");
    let known: BTreeSet<&str> = clauses.iter().map(|c| c.id.as_str()).collect();
    let resolves = ref_re
        .find_iter(&block)
        .any(|m| known.contains(m.as_str()));
    let token_re = regex::Regex::new(r"\b(INVARIANT|SURFACE)\b").expect("token regex must compile");
    let restates = token_re.is_match(&block);
    let pass = resolves && !restates;
    let detail = match (resolves, restates) {
        (true, false) => "STATUS block present, references a real clause, no regime restatement".to_string(),
        (false, _) => "STATUS block has no reference resolving to a real clause".to_string(),
        (true, true) => "STATUS block restates a regime marker token".to_string(),
    };
    check("status_block", pass, detail)
}

fn check_no_pgn_normative(clauses: &[Clause]) -> CheckResult {
    let pgn_re = regex::Regex::new(r"PGN-\d{4}").expect("pgn regex must compile");
    let offenders: Vec<&str> = clauses
        .iter()
        .filter(|c| pgn_re.is_match(&c.body))
        .map(|c| c.id.as_str())
        .collect();
    let pass = offenders.is_empty();
    let detail = if pass {
        "no clause body cites a PGN-#### token".to_string()
    } else {
        format!("clauses citing PGN normatively: {}", offenders.join(", "))
    };
    check("no_pgn_normative", pass, detail)
}

fn check_layer_sections_ordered(clauses: &[Clause]) -> CheckResult {
    let layer_index = |layer: u32| -> Option<usize> { LAYER_ORDER.iter().position(|l| *l == layer) };
    let mut last_layer_idx: Option<usize> = None;
    let mut last_n_in_layer: BTreeMap<u32, u32> = BTreeMap::new();
    let mut offenders: Vec<String> = Vec::new();
    for c in clauses {
        let idx = match layer_index(c.layer) {
            Some(i) => i,
            None => {
                offenders.push(format!("{} layer {} not in allowed set", c.id, c.layer));
                continue;
            }
        };
        if let Some(last) = last_layer_idx {
            if idx < last {
                offenders.push(format!("{} appears after a higher layer section", c.id));
            }
        }
        last_layer_idx = Some(idx.max(last_layer_idx.unwrap_or(idx)));
        let expected_n = last_n_in_layer.get(&c.layer).copied().unwrap_or(0) + 1;
        if c.n != expected_n {
            offenders.push(format!(
                "{} expected n={expected_n} within layer {} but found n={}",
                c.id, c.layer, c.n
            ));
        }
        last_n_in_layer.insert(c.layer, c.n.max(last_n_in_layer.get(&c.layer).copied().unwrap_or(0)));
    }
    let pass = offenders.is_empty();
    let detail = if pass {
        "clause ids appear in non-decreasing layer order with dense per-layer numbering".to_string()
    } else {
        format!("ordering defects: {}", offenders.join("; "))
    };
    check("layer_sections_ordered", pass, detail)
}

struct RunOutcome {
    checks: Vec<CheckResult>,
    ruled_total: usize,
    spec_bearing: usize,
    map_governance: usize,
    clause_count: usize,
    invariant_count: usize,
    surface_count: usize,
}

fn run(
    doc: &str,
    trace_text: &str,
    meadows_text: &str,
    range: (u32, u32),
    allow_regime_prose: &[String],
) -> RunOutcome {
    let clauses = parse_clauses(doc);
    let (trace_rows, _malformed) = parse_trace(trace_text);
    let expected = load_expected_ruled_ns(meadows_text);

    let checks = vec![
        check_trace_complete(&expected, &trace_rows, range),
        check_spec_bearing_has_clause(&trace_rows, range),
        check_map_governance_shape(&trace_rows, range),
        check_clause_ids_resolve(&trace_rows, &clauses),
        check_no_orphan_clause(&trace_rows, &clauses),
        check_regime_marker_unique(&clauses, allow_regime_prose),
        check_status_block(doc, &clauses),
        check_no_pgn_normative(&clauses),
        check_layer_sections_ordered(&clauses),
    ];

    let spec_bearing = trace_rows
        .iter()
        .filter(|r| r.disposition == Disposition::SpecBearing)
        .count();
    let map_governance = trace_rows
        .iter()
        .filter(|r| r.disposition == Disposition::MapGovernance)
        .count();
    let invariant_count = clauses.iter().filter(|c| c.regime == Regime::Invariant).count();
    let surface_count = clauses.iter().filter(|c| c.regime == Regime::Surface).count();

    RunOutcome {
        ruled_total: trace_rows.len(),
        spec_bearing,
        map_governance,
        clause_count: clauses.len(),
        invariant_count,
        surface_count,
        checks,
    }
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .map(|i| args.get(i + 1).unwrap_or_else(|| panic!("{flag} requires a value")).clone())
}

fn arg_values(args: &[String], flag: &str) -> Vec<String> {
    args.iter()
        .enumerate()
        .filter(|(_, a)| *a == flag)
        .map(|(i, _)| args.get(i + 1).unwrap_or_else(|| panic!("{flag} requires a value")).clone())
        .collect()
}

fn parse_range(s: &str) -> (u32, u32) {
    let (lo, hi) = s
        .split_once("..")
        .unwrap_or_else(|| panic!("--range must be of the form <lo>..<hi>, got {s}"));
    (
        lo.parse().unwrap_or_else(|e| panic!("--range lo must be a number: {e}")),
        hi.parse().unwrap_or_else(|e| panic!("--range hi must be a number: {e}")),
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let spec_path = arg_value(&args, "--spec").unwrap_or_else(|| "docs/spec/pardosa-1.0.md".to_string());
    let trace_path =
        arg_value(&args, "--trace").unwrap_or_else(|| "docs/spec/trace/ruled-trace.tsv".to_string());
    let meadows_path =
        arg_value(&args, "--meadows").unwrap_or_else(|| "scripts/data/ruled-meadows.tsv".to_string());
    let range = arg_value(&args, "--range")
        .map(|s| parse_range(&s))
        .unwrap_or((1, 244));
    let allow_regime_prose = arg_values(&args, "--allow-regime-prose");

    let doc = std::fs::read_to_string(&spec_path).unwrap_or_default();
    let trace_text = std::fs::read_to_string(&trace_path).unwrap_or_default();
    let meadows_text = std::fs::read_to_string(&meadows_path)
        .unwrap_or_else(|e| panic!("cannot read meadows tsv {meadows_path}: {e}"));

    let outcome = run(&doc, &trace_text, &meadows_text, range, &allow_regime_prose);

    let mut any_fail = false;
    for c in &outcome.checks {
        if !c.pass {
            any_fail = true;
        }
        println!(
            "CHECK\t{}\t{}\t{}",
            c.name,
            if c.pass { "PASS" } else { "FAIL" },
            c.detail
        );
    }

    println!("SUMMARY\truled_total\t{}", outcome.ruled_total);
    println!("SUMMARY\tspec_bearing\t{}", outcome.spec_bearing);
    println!("SUMMARY\tmap_governance\t{}", outcome.map_governance);
    println!("SUMMARY\tclauses\t{}", outcome.clause_count);
    println!("SUMMARY\tinvariant\t{}", outcome.invariant_count);
    println!("SUMMARY\tsurface\t{}", outcome.surface_count);
    println!("SUMMARY\tverdict\t{}", if any_fail { "FAIL" } else { "PASS" });

    std::process::exit(if any_fail { 1 } else { 0 });
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEADOWS_SMALL: &str = "1\t2\tS\tpardosa-x\tsome justification\n2\t3\tS\tpardosa-x\tsome justification\n";

    fn valid_doc() -> String {
        [
            "<!-- STATUS -->",
            "See C2.1 for the current spine.",
            "<!-- /STATUS -->",
            "",
            "#### C2.1 — INVARIANT",
            "Body text for clause one.",
            "",
            "#### C3.1 — SURFACE",
            "Body text for clause two.",
        ]
        .join("\n")
    }

    fn valid_trace() -> String {
        [
            "ruled_n\tdisposition\tclause_ids\tnote",
            "1\tSPEC-BEARING\tC2.1\t-",
            "2\tSPEC-BEARING\tC3.1\t-",
        ]
        .join("\n")
    }

    fn run_default(doc: &str, trace: &str) -> RunOutcome {
        run(doc, trace, MEADOWS_SMALL, (1, 244), &[])
    }

    fn find<'a>(outcome: &'a RunOutcome, name: &str) -> &'a CheckResult {
        outcome
            .checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no check named {name}"))
    }

    #[test]
    fn baseline_is_all_pass() {
        let outcome = run_default(&valid_doc(), &valid_trace());
        for c in &outcome.checks {
            assert!(c.pass, "expected {} to pass: {}", c.name, c.detail);
        }
    }

    #[test]
    fn empty_document_fails_status_block() {
        let outcome = run_default("", &valid_trace());
        assert!(!find(&outcome, "status_block").pass);
    }

    #[test]
    fn absent_document_fails_status_block() {
        let missing_doc = std::fs::read_to_string("/nonexistent/spec/pardosa-1.0.md").unwrap_or_default();
        assert_eq!(missing_doc, "");
        let outcome = run_default(&missing_doc, &valid_trace());
        assert!(!find(&outcome, "status_block").pass);
    }

    #[test]
    fn empty_trace_fails_trace_complete() {
        let outcome = run_default(&valid_doc(), "");
        assert!(!find(&outcome, "trace_complete").pass);
    }

    #[test]
    fn missing_ruled_n_fails_trace_complete() {
        let trace = "ruled_n\tdisposition\tclause_ids\tnote\n1\tSPEC-BEARING\tC2.1\t-\n";
        let outcome = run_default(&valid_doc(), trace);
        assert!(!find(&outcome, "trace_complete").pass);
        assert!(find(&outcome, "trace_complete").detail.contains('2'));
    }

    #[test]
    fn duplicate_ruled_n_fails_trace_complete() {
        let trace = [
            "ruled_n\tdisposition\tclause_ids\tnote",
            "1\tSPEC-BEARING\tC2.1\t-",
            "1\tSPEC-BEARING\tC2.1\t-",
            "2\tSPEC-BEARING\tC3.1\t-",
        ]
        .join("\n");
        let outcome = run_default(&valid_doc(), &trace);
        assert!(!find(&outcome, "trace_complete").pass);
        assert!(find(&outcome, "trace_complete").detail.contains("duplicate"));
    }

    #[test]
    fn dangling_clause_id_fails_clause_ids_resolve() {
        let trace = [
            "ruled_n\tdisposition\tclause_ids\tnote",
            "1\tSPEC-BEARING\tC2.99\t-",
            "2\tSPEC-BEARING\tC3.1\t-",
        ]
        .join("\n");
        let outcome = run_default(&valid_doc(), &trace);
        assert!(!find(&outcome, "clause_ids_resolve").pass);
    }

    #[test]
    fn orphan_clause_fails_no_orphan_clause() {
        let trace = [
            "ruled_n\tdisposition\tclause_ids\tnote",
            "1\tSPEC-BEARING\tC2.1\t-",
            "2\tMAP-GOVERNANCE\t-\tnot spec bearing at all, purely map governance",
        ]
        .join("\n");
        let outcome = run_default(&valid_doc(), &trace);
        assert!(!find(&outcome, "no_orphan_clause").pass);
    }

    #[test]
    fn missing_regime_marker_fails_heading_parse_and_downstream() {
        let doc = [
            "<!-- STATUS -->",
            "See C2.1 for the current spine.",
            "<!-- /STATUS -->",
            "",
            "#### C2.1 — MAYBE",
            "Body text for clause one.",
        ]
        .join("\n");
        let outcome = run_default(&doc, &valid_trace());
        assert!(!find(&outcome, "clause_ids_resolve").pass);
    }

    #[test]
    fn double_regime_marker_fails_regime_marker_unique() {
        let doc = [
            "<!-- STATUS -->",
            "See C2.1 for the current spine.",
            "<!-- /STATUS -->",
            "",
            "#### C2.1 — INVARIANT",
            "Body text mentions SURFACE by mistake.",
            "",
            "#### C3.1 — SURFACE",
            "Body text for clause two.",
        ]
        .join("\n");
        let outcome = run_default(&doc, &valid_trace());
        assert!(!find(&outcome, "regime_marker_unique").pass);
    }

    #[test]
    fn status_block_restating_regime_token_fails_status_block() {
        let doc = [
            "<!-- STATUS -->",
            "See C2.1, an INVARIANT clause, for the current spine.",
            "<!-- /STATUS -->",
            "",
            "#### C2.1 — INVARIANT",
            "Body text for clause one.",
            "",
            "#### C3.1 — SURFACE",
            "Body text for clause two.",
        ]
        .join("\n");
        let outcome = run_default(&doc, &valid_trace());
        assert!(!find(&outcome, "status_block").pass);
    }

    #[test]
    fn pgn_citation_fails_no_pgn_normative() {
        let doc = [
            "<!-- STATUS -->",
            "See C2.1 for the current spine.",
            "<!-- /STATUS -->",
            "",
            "#### C2.1 — INVARIANT",
            "This clause restates PGN-0042 normatively.",
            "",
            "#### C3.1 — SURFACE",
            "Body text for clause two.",
        ]
        .join("\n");
        let outcome = run_default(&doc, &valid_trace());
        assert!(!find(&outcome, "no_pgn_normative").pass);
    }

    #[test]
    fn out_of_order_layer_sections_fails_layer_sections_ordered() {
        let doc = [
            "<!-- STATUS -->",
            "See C3.1 for the current spine.",
            "<!-- /STATUS -->",
            "",
            "#### C3.1 — SURFACE",
            "Body text for clause two.",
            "",
            "#### C2.1 — INVARIANT",
            "Body text for clause one.",
        ]
        .join("\n");
        let outcome = run_default(&doc, &valid_trace());
        assert!(!find(&outcome, "layer_sections_ordered").pass);
    }

    #[test]
    fn allow_regime_prose_exempts_named_clause() {
        let doc = [
            "<!-- STATUS -->",
            "See C2.1 for the current spine.",
            "<!-- /STATUS -->",
            "",
            "#### C2.1 — INVARIANT",
            "This clause defines the scheme and legitimately says SURFACE.",
            "",
            "#### C3.1 — SURFACE",
            "Body text for clause two.",
        ]
        .join("\n");
        let outcome = run(&doc, &valid_trace(), MEADOWS_SMALL, (1, 244), &["C2.1".to_string()]);
        assert!(find(&outcome, "regime_marker_unique").pass);
    }

    #[test]
    fn range_mode_restricts_trace_complete_scope() {
        let meadows = "1\t2\tS\tpardosa-x\tj\n2\t3\tS\tpardosa-x\tj\n3\t4\tA\tpardosa-x\tj\n";
        let trace = [
            "ruled_n\tdisposition\tclause_ids\tnote",
            "1\tSPEC-BEARING\tC2.1\t-",
            "2\tSPEC-BEARING\tC3.1\t-",
        ]
        .join("\n");
        let outcome = run(&valid_doc(), &trace, meadows, (1, 2), &[]);
        assert!(find(&outcome, "trace_complete").pass);
    }
}
