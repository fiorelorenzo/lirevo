use std::collections::HashSet;
use std::path::PathBuf;

#[test]
fn style_spike_corpus_and_profiles_validate() {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let corpus_path = here.join("data/corpus/style-spike.jsonl");
    let profiles_path = here.join("data/profiles/style-spike.toml");

    let cases = lirevo_eval::corpus::load_jsonl(&corpus_path).expect("load style-spike corpus");
    let profiles =
        lirevo_eval::profiles::load_toml(&profiles_path).expect("load style-spike profiles");
    lirevo_eval::profiles::validate(&cases, &profiles).expect("cross-ref");

    // Every profile must exist for every language under test.
    let mut seen = HashSet::new();
    for c in &cases {
        seen.insert((c.profile.clone(), c.language.clone()));
    }
    for profile in &[
        "baseline",
        "card_casual",
        "card_formal",
        "ceiling_casual",
        "ceiling_formal",
    ] {
        for lang in &["it", "en"] {
            assert!(
                seen.contains(&((*profile).to_string(), (*lang).to_string())),
                "missing coverage: profile={profile} lang={lang}",
            );
        }
    }

    // The experiment is only readable if each source case appears in all three
    // arms with a byte-identical `expected`. Diverging references would make
    // the arms incomparable while still producing plausible-looking numbers.
    let mut by_stem: std::collections::HashMap<String, Vec<&lirevo_eval::corpus::TestCase>> =
        std::collections::HashMap::new();
    for c in &cases {
        let stem =
            c.id.rsplit_once('-')
                .expect("case id must end in -<arm>")
                .0
                .to_string();
        by_stem.entry(stem).or_default().push(c);
    }
    assert!(!by_stem.is_empty(), "corpus is empty");
    for (stem, group) in &by_stem {
        assert_eq!(group.len(), 3, "stem {stem} must have exactly 3 arms");

        let expected = &group[0].expected;
        for c in group {
            assert_eq!(
                &c.expected, expected,
                "arms of {stem} disagree on `expected`; they would not be comparable",
            );
        }

        // The card and ceiling arms must carry the style of their OWN source.
        // A casual card scored against a formal reference would report "the
        // card fails" from an authoring error rather than from the model.
        let style = if stem.contains("chat_casual") {
            "casual"
        } else if stem.contains("mail_formal") {
            "formal"
        } else {
            panic!("stem {stem} came from an unexpected source profile");
        };
        let arms: HashSet<&str> = group.iter().map(|c| c.profile.as_str()).collect();
        let want = HashSet::from([
            "baseline",
            if style == "casual" {
                "card_casual"
            } else {
                "card_formal"
            },
            if style == "casual" {
                "ceiling_casual"
            } else {
                "ceiling_formal"
            },
        ]);
        assert_eq!(
            arms, want,
            "stem {stem} ({style}) has the wrong arm profiles"
        );
    }
}
