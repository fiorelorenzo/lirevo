use std::collections::HashSet;
use std::path::PathBuf;

#[test]
fn seed_corpus_and_profiles_validate() {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let corpus_path = here.join("data/corpus/v1-seed.jsonl");
    let profiles_path = here.join("data/profiles/v1.toml");

    let cases = lda_eval::corpus::load_jsonl(&corpus_path).expect("load corpus");
    let profiles = lda_eval::profiles::load_toml(&profiles_path).expect("load profiles");
    lda_eval::profiles::validate(&cases, &profiles).expect("cross-ref");

    let mut seen = HashSet::new();
    for c in &cases {
        seen.insert((c.profile.clone(), c.language.clone()));
    }
    for profile in &[
        "plain",
        "mail_formal",
        "mail_casual",
        "chat_casual",
        "slack_brief",
        "code_comment",
    ] {
        for lang in &["en", "it", "fr", "de", "es"] {
            assert!(
                seen.contains(&((*profile).to_string(), (*lang).to_string())),
                "missing coverage: profile={profile} lang={lang}",
            );
        }
    }

    assert_eq!(
        cases.len(),
        60,
        "expected 60 seed cases, got {}",
        cases.len()
    );
}
