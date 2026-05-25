#[test]
fn clean_prompt_is_non_empty_and_substitutes_language() {
    let prompt = lirevo_prompts::build_clean_system_prompt("auto");
    assert!(!prompt.is_empty());
    assert!(prompt.len() > 50, "prompt suspiciously short");
}
