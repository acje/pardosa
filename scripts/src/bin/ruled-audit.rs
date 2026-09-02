//! Audit RULED n cross-references in the pardosa-jn1 bd corpus.
//!
//! Usage: cargo run --bin ruled-audit -- [--all] [--verdict OK|MISMATCH|UNPAIRED|OUTOFRANGE]
//!        [--owners <tsv>] [--cache <json>]
//! Output: tab-separated RANGE / RANGEGAP / RANGEOVERLAP / CITE / SUMMARY records.
//!
//! Ownership of each RULED number is read from a curated TSV (`ticket<TAB>lo<TAB>hi`),
//! not derived from close_reason text: a close_reason states both the rulings the
//! ticket owns and the rulings it cites, so derivation cannot separate them.

use regex::Regex;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::process::Command;

#[derive(Debug, Deserialize)]
struct ListEntry {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ShowEntry {
    id: String,
    description: Option<String>,
    close_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CommentEntry {
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Ok,
    Mismatch,
    Unpaired,
    OutOfRange,
}

impl Verdict {
    fn as_str(&self) -> &'static str {
        match self {
            Verdict::Ok => "OK",
            Verdict::Mismatch => "MISMATCH",
            Verdict::Unpaired => "UNPAIRED",
            Verdict::OutOfRange => "OUTOFRANGE",
        }
    }

    fn parse(s: &str) -> Verdict {
        match s {
            "OK" => Verdict::Ok,
            "MISMATCH" => Verdict::Mismatch,
            "UNPAIRED" => Verdict::Unpaired,
            "OUTOFRANGE" => Verdict::OutOfRange,
            other => panic!("unknown verdict filter {other}; expected OK|MISMATCH|UNPAIRED|OUTOFRANGE"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum FieldKind {
    Description,
    CloseReason,
    Comment,
}

impl FieldKind {
    fn as_str(&self) -> &'static str {
        match self {
            FieldKind::Description => "description",
            FieldKind::CloseReason => "close_reason",
            FieldKind::Comment => "comment",
        }
    }
}

struct Source {
    bead_id: String,
    fields: Vec<(FieldKind, String)>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct CachedField {
    kind: String,
    text: String,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct CachedSource {
    bead_id: String,
    fields: Vec<CachedField>,
}

fn parse_field_kind(s: &str) -> FieldKind {
    match s {
        "description" => FieldKind::Description,
        "close_reason" => FieldKind::CloseReason,
        "comment" => FieldKind::Comment,
        other => panic!("unknown cached field kind {other}"),
    }
}

fn load_owners(path: &str) -> BTreeMap<u32, Vec<String>> {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read owners table {path}: {e}"));
    let mut owners: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for (lineno, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 3 {
            panic!("owners table {path}:{} expected 3 tab-separated columns, got {}", lineno + 1, cols.len());
        }
        let lo: u32 = cols[1]
            .parse()
            .unwrap_or_else(|e| panic!("owners table {path}:{} bad lo: {e}", lineno + 1));
        let hi: u32 = cols[2]
            .parse()
            .unwrap_or_else(|e| panic!("owners table {path}:{} bad hi: {e}", lineno + 1));
        if hi < lo {
            panic!("owners table {path}:{} hi {hi} precedes lo {lo}", lineno + 1);
        }
        for n in lo..=hi {
            owners.entry(n).or_default().push(cols[0].to_string());
        }
    }
    if owners.is_empty() {
        panic!("owners table {path} yielded no entries");
    }
    owners
}

fn bd_json(args: &[&str]) -> String {
    let output = Command::new("bd")
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn bd {args:?}: {e}"));
    if !output.status.success() {
        panic!(
            "bd {args:?} exited non-zero: stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8(output.stdout).unwrap_or_else(|e| panic!("bd {args:?} produced non-utf8 stdout: {e}"))
}

fn fetch_show(id: &str) -> ShowEntry {
    let raw = bd_json(&["show", id, "--json"]);
    let mut arr: Vec<ShowEntry> =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("bd show {id} --json parse failed: {e}; raw={raw}"));
    if arr.is_empty() {
        panic!("bd show {id} --json returned empty array");
    }
    arr.remove(0)
}

fn fetch_comments(id: &str) -> Vec<String> {
    let output = Command::new("bd")
        .args(["comments", id, "--json"])
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn bd comments {id}: {e}"));
    if !output.status.success() {
        return Vec::new();
    }
    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    let parsed: Result<Vec<CommentEntry>, _> = serde_json::from_str(&raw);
    match parsed {
        Ok(entries) => entries.into_iter().map(|c| c.text).collect(),
        Err(_) => Vec::new(),
    }
}

fn ruled_ranges_re() -> Regex {
    Regex::new(r"RULED\s+(\d+)(?:\s*[-\u{2013}]\s*(\d+))?").expect("ruled range regex must compile")
}

fn ticket_ref_re() -> Regex {
    Regex::new(r"(?:pardosa-)?(jn1\.\d+)|(pardosa-abi)").expect("ticket ref regex must compile")
}

fn normalize_ticket(jn1_capture: Option<&str>, abi_capture: Option<&str>) -> String {
    if let Some(abi) = abi_capture {
        return abi.to_string();
    }
    if let Some(jn1) = jn1_capture {
        return format!("pardosa-{jn1}");
    }
    panic!("ticket ref regex matched with no capture group populated");
}

fn last_ticket_ref_before(text: &str, byte_pos: usize) -> Option<(String, usize)> {
    let window_start = text
        .char_indices()
        .rev()
        .find(|(i, _)| *i <= byte_pos.saturating_sub(120))
        .map(|(i, _)| i)
        .unwrap_or(0);
    let window = &text[window_start..byte_pos];
    let re = ticket_ref_re();
    re.captures_iter(window).last().map(|c| {
        let whole = c.get(0).expect("capture group 0 must exist");
        let gap = window.len() - whole.end();
        (
            normalize_ticket(c.get(1).map(|m| m.as_str()), c.get(2).map(|m| m.as_str())),
            gap,
        )
    })
}

fn context_60(text: &str, byte_pos: usize, match_len: usize) -> String {
    let start = text
        .char_indices()
        .rev()
        .find(|(i, _)| *i <= byte_pos.saturating_sub(60))
        .map(|(i, _)| i)
        .unwrap_or(0);
    let end_target = byte_pos + match_len + 60;
    let end = text
        .char_indices()
        .find(|(i, _)| *i >= end_target)
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    let slice = &text[start..end.min(text.len())];
    slice.replace('\n', " ").replace('\t', " ")
}

fn collapse_to_ranges(mut nums: Vec<u32>) -> String {
    nums.sort_unstable();
    nums.dedup();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < nums.len() {
        let mut j = i;
        while j + 1 < nums.len() && nums[j + 1] == nums[j] + 1 {
            j += 1;
        }
        if i == j {
            out.push(nums[i].to_string());
        } else {
            out.push(format!("{}-{}", nums[i], nums[j]));
        }
        i = j + 1;
    }
    out.join(",")
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .map(|i| args.get(i + 1).unwrap_or_else(|| panic!("{flag} requires a value")).clone())
}

fn fetch_corpus(parent_id: &str) -> Vec<Source> {
    let list_raw = bd_json(&["list", "--parent", parent_id, "--all", "--json"]);
    let entries: Vec<ListEntry> =
        serde_json::from_str(&list_raw).unwrap_or_else(|e| panic!("bd list --json parse failed: {e}; raw={list_raw}"));

    let mut sources: Vec<Source> = Vec::new();
    let mut ids: Vec<String> = entries.into_iter().map(|e| e.id).collect();
    ids.push(parent_id.to_string());

    for id in &ids {
        let show = fetch_show(id);
        let mut fields = Vec::new();
        if let Some(d) = &show.description {
            fields.push((FieldKind::Description, d.clone()));
        }
        if let Some(cr) = &show.close_reason {
            fields.push((FieldKind::CloseReason, cr.clone()));
        }
        for text in fetch_comments(id) {
            fields.push((FieldKind::Comment, text));
        }
        sources.push(Source {
            bead_id: show.id.clone(),
            fields,
        });
    }
    sources
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let show_all = args.iter().any(|a| a == "--all");
    let verdict_filter = args
        .iter()
        .position(|a| a == "--verdict")
        .map(|i| Verdict::parse(args.get(i + 1).expect("--verdict requires a value")));
    let owners_path = arg_value(&args, "--owners").unwrap_or_else(|| "scripts/data/ruled-owners.tsv".to_string());
    let cache_path = arg_value(&args, "--cache");

    let parent_id = "pardosa-jn1";
    let owners = load_owners(&owners_path);

    let sources: Vec<Source> = match &cache_path {
        Some(path) if std::path::Path::new(path).exists() => {
            let raw = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read cache {path}: {e}"));
            let cached: Vec<CachedSource> =
                serde_json::from_str(&raw).unwrap_or_else(|e| panic!("cache {path} parse failed: {e}"));
            cached
                .into_iter()
                .map(|c| Source {
                    bead_id: c.bead_id,
                    fields: c
                        .fields
                        .into_iter()
                        .map(|f| (parse_field_kind(&f.kind), f.text))
                        .collect(),
                })
                .collect()
        }
        Some(path) => {
            let fetched = fetch_corpus(parent_id);
            let cached: Vec<CachedSource> = fetched
                .iter()
                .map(|s| CachedSource {
                    bead_id: s.bead_id.clone(),
                    fields: s
                        .fields
                        .iter()
                        .map(|(k, t)| CachedField {
                            kind: k.as_str().to_string(),
                            text: t.clone(),
                        })
                        .collect(),
                })
                .collect();
            let encoded = serde_json::to_string(&cached).expect("cache must serialize");
            std::fs::write(path, encoded).unwrap_or_else(|e| panic!("failed to write cache {path}: {e}"));
            fetched
        }
        None => fetch_corpus(parent_id),
    };

    let ruled_re = ruled_ranges_re();

    let mut per_ticket_owned: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for (num, tickets) in &owners {
        for t in tickets {
            per_ticket_owned.entry(t.clone()).or_default().push(*num);
        }
    }

    let mut range_rows: Vec<(u32, String, String)> = per_ticket_owned
        .iter()
        .map(|(ticket, nums)| {
            let min = *nums.iter().min().expect("owned set must be non-empty");
            (min, ticket.clone(), collapse_to_ranges(nums.clone()))
        })
        .collect();
    range_rows.sort_by_key(|(min, _, _)| *min);

    for (_, ticket, collapsed) in &range_rows {
        println!("RANGE\t{ticket}\t{collapsed}");
    }

    let max_num = owners.keys().copied().max().unwrap_or(0);
    for n in 1..=max_num {
        if !owners.contains_key(&n) {
            println!("RANGEGAP\t{n}");
        }
    }
    for (n, tickets) in &owners {
        if tickets.len() > 1 {
            for i in 0..tickets.len() {
                for j in (i + 1)..tickets.len() {
                    println!("RANGEOVERLAP\t{n}\t{}\t{}", tickets[i], tickets[j]);
                }
            }
        }
    }

    let mut total = 0u32;
    let mut ok = 0u32;
    let mut mismatch = 0u32;
    let mut unpaired = 0u32;
    let mut outofrange = 0u32;
    let mut cite_lines: Vec<String> = Vec::new();

    for source in &sources {
        for (kind, text) in &source.fields {
            for cap in ruled_re.captures_iter(text) {
                let whole = cap.get(0).expect("capture group 0 must exist");
                let lo: u32 = cap[1].parse().expect("RULED number must parse as u32");
                let hi: u32 = cap
                    .get(2)
                    .map(|m| m.as_str().parse().expect("RULED range end must parse as u32"))
                    .unwrap_or(lo);
                let paired = last_ticket_ref_before(text, whole.start());
                let ctx = context_60(text, whole.start(), whole.len());
                for n in lo..=hi {
                    total += 1;
                    let owner_set = owners.get(&n);
                    let owner_str = owner_set
                        .map(|v| {
                            let mut sorted = v.clone();
                            sorted.sort();
                            sorted.dedup();
                            sorted.join(",")
                        })
                        .unwrap_or_else(|| "-".to_string());
                    let verdict = match (&paired, owner_set) {
                        (_, None) => Verdict::OutOfRange,
                        (None, Some(_)) => Verdict::Unpaired,
                        (Some((p, _)), Some(owns)) if owns.contains(p) => Verdict::Ok,
                        (Some(_), Some(_)) => Verdict::Mismatch,
                    };
                    match verdict {
                        Verdict::Ok => ok += 1,
                        Verdict::Mismatch => mismatch += 1,
                        Verdict::Unpaired => unpaired += 1,
                        Verdict::OutOfRange => outofrange += 1,
                    }
                    cite_lines.push(format!(
                        "CITE\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        verdict.as_str(),
                        source.bead_id,
                        kind.as_str(),
                        paired
                            .as_ref()
                            .map(|(p, _)| p.clone())
                            .unwrap_or_else(|| "-".to_string()),
                        paired
                            .as_ref()
                            .map(|(_, g)| g.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        n,
                        owner_str,
                        ctx,
                    ));
                }
            }
        }
    }

    for line in &cite_lines {
        let fields: Vec<&str> = line.split('\t').collect();
        let verdict_field = fields.get(1).copied().unwrap_or("");
        let is_ok = verdict_field == "OK";
        if let Some(filter) = verdict_filter {
            if verdict_field != filter.as_str() {
                continue;
            }
        } else if is_ok && !show_all {
            continue;
        }
        println!("{line}");
    }

    println!(
        "SUMMARY\ttotal_mentions={total}\tok={ok}\tmismatch={mismatch}\tunpaired={unpaired}\toutofrange={outofrange}"
    );
}
