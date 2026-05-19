//! chrF — character n-gram F-score (Popović 2015).
//!
//! Default parameters: max n=6, beta=2.0 (recall-weighted). Operates on
//! Unicode scalar values; lowercases and collapses whitespace before
//! n-gram extraction so trivial casing/spacing differences don't tank scores.

use std::collections::HashMap;

fn normalize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn ngrams(s: &str, n: usize) -> HashMap<String, u32> {
    let chars: Vec<char> = s.chars().collect();
    let mut out: HashMap<String, u32> = HashMap::new();
    if chars.len() < n {
        return out;
    }
    for window in chars.windows(n) {
        let key: String = window.iter().collect();
        *out.entry(key).or_insert(0) += 1;
    }
    out
}

fn f_score(prec: f64, rec: f64, beta: f64) -> f64 {
    if prec + rec == 0.0 {
        return 0.0;
    }
    let b2 = beta * beta;
    (1.0 + b2) * prec * rec / (b2 * prec + rec)
}

fn precision_recall(cand: &HashMap<String, u32>, ref_: &HashMap<String, u32>) -> (f64, f64) {
    let mut overlap: u32 = 0;
    for (k, v) in cand {
        if let Some(r) = ref_.get(k) {
            overlap += (*v).min(*r);
        }
    }
    let cand_total: u32 = cand.values().sum();
    let ref_total: u32 = ref_.values().sum();
    let prec = if cand_total == 0 {
        0.0
    } else {
        f64::from(overlap) / f64::from(cand_total)
    };
    let rec = if ref_total == 0 {
        0.0
    } else {
        f64::from(overlap) / f64::from(ref_total)
    };
    (prec, rec)
}

#[must_use]
pub fn chrf(candidate: &str, reference: &str, max_n: usize, beta: f64) -> f64 {
    let c = normalize(candidate);
    let r = normalize(reference);
    if c.is_empty() || r.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut n_used = 0_u32;
    for n in 1..=max_n {
        let cg = ngrams(&c, n);
        let rg = ngrams(&r, n);
        if cg.is_empty() && rg.is_empty() {
            continue;
        }
        let (p, r_) = precision_recall(&cg, &rg);
        sum += f_score(p, r_, beta);
        n_used += 1;
    }
    if n_used == 0 {
        0.0
    } else {
        sum / f64::from(n_used)
    }
}

#[cfg(test)]
mod tests {
    use super::chrf;

    #[test]
    fn identical_strings_score_one() {
        let s = chrf("hello world", "hello world", 6, 2.0);
        assert!((s - 1.0).abs() < 1e-6, "got {s}");
    }

    #[test]
    fn completely_different_strings_score_low() {
        let s = chrf("hello world", "xyzqrs", 6, 2.0);
        assert!(s < 0.1, "got {s}");
    }

    #[test]
    fn paraphrase_scores_intermediate() {
        let s = chrf("the cat sat on the mat", "a cat is on a mat", 6, 2.0);
        assert!(s > 0.3 && s < 0.9, "got {s}");
    }

    #[test]
    fn empty_strings_score_zero() {
        assert!(chrf("", "anything", 6, 2.0).abs() < f64::EPSILON);
        assert!(chrf("anything", "", 6, 2.0).abs() < f64::EPSILON);
    }
}
