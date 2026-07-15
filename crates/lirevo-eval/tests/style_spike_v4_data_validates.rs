use std::collections::HashSet;
use std::path::PathBuf;

#[test]
fn style_spike_v4_corpus_and_profiles_validate() {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let corpus_path = here.join("data/corpus/style-spike-v4.jsonl");
    let profiles_path = here.join("data/profiles/style-spike-v4.toml");

    let cases = lirevo_eval::corpus::load_jsonl(&corpus_path).expect("load style-spike-v4 corpus");
    let profiles =
        lirevo_eval::profiles::load_toml(&profiles_path).expect("load style-spike-v4 profiles");
    lirevo_eval::profiles::validate(&cases, &profiles).expect("cross-ref");

    // Every profile must exist for every language under test.
    let mut seen = HashSet::new();
    for c in &cases {
        seen.insert((c.profile.clone(), c.language.clone()));
    }
    for profile in &[
        "baseline",
        "conflict_casual",
        "conflict_formal",
        "amended_casual",
        "amended_formal",
    ] {
        for lang in &["it", "en"] {
            assert!(
                seen.contains(&((*profile).to_string(), (*lang).to_string())),
                "missing coverage: profile={profile} lang={lang}",
            );
        }
    }

    // The anti-drift guard, same as v2: `baseline` must be the real shipped
    // prompt, byte-identical to `build_clean_system_prompt`, for both
    // dictation languages under test.
    for lang in &["it", "en"] {
        let shipped = lirevo_prompts::build_clean_system_prompt(lang);
        let baseline = &profiles["baseline"].system_prompts[*lang];
        assert_eq!(
            baseline, &shipped,
            "baseline[{lang}] has drifted from lirevo_prompts::build_clean_system_prompt",
        );
    }

    // `conflict_*` must start with that exact baseline — it is baseline + the
    // prose card, reproducing v3's confounded arm in-run as the control.
    for profile in ["conflict_casual", "conflict_formal"] {
        for lang in &["it", "en"] {
            let shipped = lirevo_prompts::build_clean_system_prompt(lang);
            let prompt = &profiles[profile].system_prompts[*lang];
            assert!(
                prompt.starts_with(&shipped),
                "{profile}[{lang}] does not start with the shipped baseline prompt",
            );
            assert!(
                prompt.len() > shipped.len(),
                "{profile}[{lang}] is not longer than the baseline — no card appended",
            );
        }
    }

    // `amended_*` must NOT contain the old preservation clause and MUST
    // contain the new one — the whole point of v4. A silent no-op replace
    // would make `amended` identical to `conflict` and the run would
    // measure baseline three times while looking fine.
    for profile in ["amended_casual", "amended_formal"] {
        for lang in &["it", "en"] {
            let prompt = &profiles[profile].system_prompts[*lang];
            assert!(
                !prompt.contains("do not paraphrase"),
                "{profile}[{lang}] still forbids paraphrase — clause surgery failed",
            );
            assert!(
                prompt.contains("adapt tone, register"),
                "{profile}[{lang}] is missing the amended clause",
            );
        }
    }

    // `amended` and `conflict` must end with the SAME card block. If the card
    // differs between the two arms, the comparison measures two variables
    // instead of one and the entire point of v4 is void.
    for (conflict_profile, amended_profile) in [
        ("conflict_casual", "amended_casual"),
        ("conflict_formal", "amended_formal"),
    ] {
        for lang in &["it", "en"] {
            let shipped = lirevo_prompts::build_clean_system_prompt(lang);
            let conflict_prompt = &profiles[conflict_profile].system_prompts[*lang];
            let amended_prompt = &profiles[amended_profile].system_prompts[*lang];
            let card = &conflict_prompt[shipped.len()..];
            assert!(
                !card.trim().is_empty(),
                "{conflict_profile}[{lang}] has an empty card block",
            );
            assert!(
                amended_prompt.ends_with(card),
                "{amended_profile}[{lang}] does not end with the same card block as {conflict_profile}[{lang}]",
            );
        }
    }

    // The experiment is only readable if each source case appears in all
    // three arms with a byte-identical `expected`. Diverging references
    // would make the arms incomparable while still producing
    // plausible-looking numbers.
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

        // The conflict and amended arms must carry the style of their OWN
        // source. A casual card scored against a formal reference would
        // report "the card fails" from an authoring error rather than from
        // the model.
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
                "conflict_casual"
            } else {
                "conflict_formal"
            },
            if style == "casual" {
                "amended_casual"
            } else {
                "amended_formal"
            },
        ]);
        assert_eq!(
            arms, want,
            "stem {stem} ({style}) has the wrong arm profiles"
        );
    }
}
