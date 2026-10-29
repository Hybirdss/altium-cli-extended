//! Designator ordering and range compression (`R1, R2, R3, R7` → `R1-R3, R7`).

use std::cmp::Ordering;

/// Split `R12A` into (`R`, 12, `A`). Designators without a number sort by text.
pub fn split(d: &str) -> (String, Option<u64>, String) {
    let start = d.find(|c: char| c.is_ascii_digit());
    let Some(s) = start else { return (d.to_owned(), None, String::new()) };
    let end = d[s..].find(|c: char| !c.is_ascii_digit()).map(|e| s + e).unwrap_or(d.len());
    (d[..s].to_owned(), d[s..end].parse().ok(), d[end..].to_owned())
}

/// Natural order: prefix, then number, then suffix.
pub fn cmp(a: &str, b: &str) -> Ordering {
    let (pa, na, sa) = split(a);
    let (pb, nb, sb) = split(b);
    pa.to_ascii_uppercase()
        .cmp(&pb.to_ascii_uppercase())
        .then(na.cmp(&nb))
        .then(sa.cmp(&sb))
        .then(a.cmp(b))
}

pub fn sort(v: &mut [String]) {
    v.sort_by(|a, b| cmp(a, b));
}

/// Compress consecutive designators with the same prefix and no suffix into ranges.
pub fn compress(sorted: &[String]) -> String {
    let mut out: Vec<String> = vec![];
    let mut i = 0;
    while i < sorted.len() {
        let (p, n, s) = split(&sorted[i]);
        let mut j = i;
        if let (Some(n0), true) = (n, s.is_empty()) {
            let mut last = n0;
            while j + 1 < sorted.len() {
                let (p2, n2, s2) = split(&sorted[j + 1]);
                if p2 == p && s2.is_empty() && n2 == Some(last + 1) {
                    last += 1;
                    j += 1;
                } else {
                    break;
                }
            }
        }
        if j >= i + 2 {
            out.push(format!("{}-{}", sorted[i], sorted[j]));
        } else {
            out.extend(sorted[i..=j].iter().cloned());
        }
        i = j + 1;
    }
    out.join(", ")
}

/// Designator still carries Altium's unannotated marker (`R?`, `U?A`).
pub fn unannotated(d: &str) -> bool {
    d.contains('?') || d.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_and_compress() {
        let mut v: Vec<String> = ["R10", "R2", "R1", "R3", "C1", "R7", "U1A"].iter().map(|s| s.to_string()).collect();
        sort(&mut v);
        assert_eq!(v, ["C1", "R1", "R2", "R3", "R7", "R10", "U1A"]);
        assert_eq!(compress(&v), "C1, R1-R3, R7, R10, U1A");
        assert_eq!(compress(&["R1".into(), "R2".into()]), "R1, R2");
        assert!(unannotated("R?"));
    }
}
