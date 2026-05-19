//! Length-ratio metric: |candidate| / |reference| in characters.
//! Flags suspiciously short or long outputs without judging quality.

#[must_use]
pub fn length_ratio(candidate: &str, reference: &str) -> f64 {
    let r = reference.chars().count();
    if r == 0 {
        return 0.0;
    }
    let c = candidate.chars().count();
    // char counts for refiner I/O are well under 2^53; f64 conversion is lossless.
    #[allow(clippy::cast_precision_loss)]
    {
        c as f64 / r as f64
    }
}

#[cfg(test)]
mod tests {
    use super::length_ratio;

    #[test]
    fn equal_lengths_ratio_one() {
        assert!((length_ratio("hello", "world") - 1.0).abs() < 1e-9);
    }

    #[test]
    fn empty_reference_returns_zero() {
        assert!(length_ratio("hello", "").abs() < f64::EPSILON);
    }
}
