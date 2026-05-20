//! Small cross-module helpers.

/// Remove `<think>…</think>` blocks (and trim leftover whitespace) from a
/// model's raw output. Used to extract the canonical answer from Qwen3 /
/// Qwen3.5 hybrid models that emit a chain-of-thought prelude. Non-thinking
/// models don't produce these tags so the function is a no-op for them.
#[must_use]
pub fn strip_think(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        if let Some(end_rel) = rest[start..].find("</think>") {
            let after = start + end_rel + "</think>".len();
            rest = &rest[after..];
        } else {
            // Unterminated <think>: drop everything from the tag onward.
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::strip_think;

    #[test]
    fn strip_think_removes_balanced_block() {
        let s = "<think>\nI reason here.\n</think>\n\nFinal answer.";
        assert_eq!(strip_think(s), "Final answer.");
    }

    #[test]
    fn strip_think_handles_empty_block() {
        let s = "<think>\n\n</think>\n\nOK.";
        assert_eq!(strip_think(s), "OK.");
    }

    #[test]
    fn strip_think_noop_when_absent() {
        let s = "Just an answer.";
        assert_eq!(strip_think(s), "Just an answer.");
    }

    #[test]
    fn strip_think_handles_multiple_blocks() {
        let s = "<think>a</think>between<think>b</think>after";
        assert_eq!(strip_think(s), "betweenafter");
    }

    #[test]
    fn strip_think_drops_unterminated_block() {
        let s = "Answer.\n<think>truncated...";
        assert_eq!(strip_think(s), "Answer.");
    }
}
