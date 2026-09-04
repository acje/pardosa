use std::path::Path;

struct Reassignment {
    ruled_n: &'static str,
    new_ids: &'static str,
}

const REASSIGNMENTS: &[Reassignment] = &[
    Reassignment { ruled_n: "68", new_ids: "C4.10;C4.11" },
    Reassignment { ruled_n: "238", new_ids: "C4.12" },
    Reassignment { ruled_n: "239", new_ids: "C4.12" },
];

const SHIFT_LOW: u32 = 11;
const SHIFT_HIGH: u32 = 22;
const SHIFT_DELTA: u32 = 2;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let apply = args.iter().any(|a| a == "--apply");
    let positional: Vec<&String> = args.iter().filter(|a| a.as_str() != "--apply").collect();
    let spec_path = positional
        .first()
        .expect("usage: c4-split-renumber <spec.md> <trace.tsv> [--apply]");
    let trace_path = positional
        .get(1)
        .expect("usage: c4-split-renumber <spec.md> <trace.tsv> [--apply]");

    let spec_src = std::fs::read_to_string(spec_path)
        .unwrap_or_else(|e| panic!("cannot read spec file {}: {}", spec_path, e));
    let trace_src = std::fs::read_to_string(trace_path)
        .unwrap_or_else(|e| panic!("cannot read trace file {}: {}", trace_path, e));

    check_idempotence(&spec_src, &trace_src);

    let other_c_before = other_clause_ids(&spec_src);

    let (spec_out, spec_shifted) = shift_clause_ids(&spec_src);
    let (trace_shifted_src, trace_shifted_cells) = shift_clause_ids(&trace_src);
    let (trace_out, rows_reassigned) = reassign_rows(&trace_shifted_src);

    let other_c_after = other_clause_ids(&spec_out);
    if other_c_before != other_c_after {
        panic!(
            "non-C4 clause ids changed: before={:?} after={:?}",
            other_c_before, other_c_after
        );
    }

    println!("COUNT\tspec_headings_shifted\t{}", spec_shifted);
    println!("COUNT\ttrace_cells_shifted\t{}", trace_shifted_cells);
    println!("COUNT\ttrace_rows_reassigned\t{}", rows_reassigned);

    if !apply {
        println!("OK\tdry-run\tno files written; pass --apply to write");
        return;
    }

    write_file(Path::new(spec_path), &spec_out);
    write_file(Path::new(trace_path), &trace_out);
    println!("OK\tapplied\t{} {}", spec_path, trace_path);
}

fn write_file(path: &Path, contents: &str) {
    std::fs::write(path, contents)
        .unwrap_or_else(|e| panic!("cannot write {}: {}", path.display(), e));
}

fn check_idempotence(spec_src: &str, trace_src: &str) {
    if find_clause_ids(spec_src).iter().any(|n| *n == 23 || *n == 24) {
        panic!(
            "idempotence guard: spec already contains C4.23 or C4.24; refusing to double-shift"
        );
    }
    let still_pending = REASSIGNMENTS.iter().any(|r| {
        trace_src.lines().any(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            fields.len() > 2 && fields[0] == r.ruled_n && fields[2] == "C4.10"
        })
    });
    if !still_pending {
        panic!(
            "idempotence guard: no C4.10-only trace row left among ruled_n 68/238/239; refusing to re-run"
        );
    }
}

fn find_clause_ids(src: &str) -> Vec<u32> {
    let bytes = src.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        if &bytes[i..i + 3] == b"C4." {
            let mut j = i + 3;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 3 {
                let boundary_ok = j == bytes.len() || !bytes[j].is_ascii_digit();
                if boundary_ok {
                    let n: u32 = src[i + 3..j].parse().expect("digits must parse");
                    found.push(n);
                }
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    found
}

fn other_clause_ids(src: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for prefix in ["C1.", "C2.", "C3.", "C5.", "C6."] {
        let bytes = src.as_bytes();
        let pbytes = prefix.as_bytes();
        let mut i = 0;
        while i + pbytes.len() <= bytes.len() {
            if &bytes[i..i + pbytes.len()] == pbytes {
                let mut j = i + pbytes.len();
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + pbytes.len() {
                    let boundary_ok = j == bytes.len() || !bytes[j].is_ascii_digit();
                    if boundary_ok {
                        ids.push(src[i..j].to_string());
                    }
                }
                i = j.max(i + 1);
            } else {
                i += 1;
            }
        }
    }
    ids.sort();
    ids
}

fn shift_clause_ids(src: &str) -> (String, usize) {
    let mut shifted = 0;
    let mut n = SHIFT_HIGH;
    let mut current = src.to_string();
    loop {
        let from = format!("C4.{}", n);
        let to = format!("C4.{}", n + SHIFT_DELTA);
        let (replaced, count) = replace_token_exact(&current, &from, &to);
        current = replaced;
        shifted += count;
        if n == SHIFT_LOW {
            break;
        }
        n -= 1;
    }
    (current, shifted)
}

fn replace_token_exact(src: &str, from: &str, to: &str) -> (String, usize) {
    let bytes = src.as_bytes();
    let fbytes = from.as_bytes();
    let tbytes = to.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut count = 0;
    let mut i = 0;
    while i < bytes.len() {
        if i + fbytes.len() <= bytes.len() && &bytes[i..i + fbytes.len()] == fbytes {
            let right_ok = i + fbytes.len() == bytes.len()
                || !bytes[i + fbytes.len()].is_ascii_digit();
            if right_ok {
                out.extend_from_slice(tbytes);
                count += 1;
                i += fbytes.len();
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let s = String::from_utf8(out).expect("byte-for-byte copy of valid UTF-8 stays valid UTF-8");
    (s, count)
}

fn reassign_rows(trace_src: &str) -> (String, usize) {
    let mut reassigned = 0;
    let mut out_lines: Vec<String> = Vec::new();
    for line in trace_src.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let mut replaced_line = None;
        if fields.len() > 2 {
            for r in REASSIGNMENTS {
                if fields[0] == r.ruled_n && fields[2] == "C4.10" {
                    let mut new_fields = fields.clone();
                    new_fields[2] = r.new_ids;
                    replaced_line = Some(new_fields.join("\t"));
                    reassigned += 1;
                    break;
                }
            }
        }
        out_lines.push(replaced_line.unwrap_or_else(|| line.to_string()));
    }
    let mut out = out_lines.join("\n");
    if trace_src.ends_with('\n') {
        out.push('\n');
    }
    (out, reassigned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_boundary_c4_2_does_not_match_inside_c4_22() {
        let (out, count) = replace_token_exact("see C4.22 and C4.2 here", "C4.2", "C4.4");
        assert_eq!(count, 1);
        assert_eq!(out, "see C4.22 and C4.4 here");
    }

    #[test]
    fn descending_shift_avoids_collision() {
        let src = "C4.11 C4.12 C4.13";
        let (out, count) = shift_clause_ids(src);
        assert_eq!(out, "C4.13 C4.14 C4.15");
        assert_eq!(count, 3);
    }

    #[test]
    fn shift_is_idempotence_detectable() {
        let src = "#### C4.24 — INVARIANT";
        assert!(find_clause_ids(src).contains(&24));
    }

    #[test]
    fn other_clause_layers_untouched_by_shift() {
        let src = "C1.3 C4.11 C6.9";
        let (out, _) = shift_clause_ids(src);
        assert_eq!(out, "C1.3 C4.13 C6.9");
    }

    #[test]
    fn reassignment_uses_file_separator_not_comma() {
        let trace = "68\tSPEC-BEARING\tC4.10\ttext\n238\tSPEC-BEARING\tC4.10\ttext\n";
        let (out, count) = reassign_rows(trace);
        assert_eq!(count, 2);
        assert!(out.contains("C4.10;C4.11"));
        assert!(out.contains("\tC4.12\t"));
    }
}
