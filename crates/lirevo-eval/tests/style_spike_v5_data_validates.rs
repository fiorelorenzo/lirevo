use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// `(raw, final)` pairs for one (style, language) cell.
type ExamplePairs = Vec<(String, String)>;
/// style -> language -> example pairs.
type ExamplesByStyle = HashMap<&'static str, HashMap<&'static str, ExamplePairs>>;

/// Mirrors the `EXAMPLES` dict in `scripts/derive-style-spike-v5.py`. Duplicating
/// the example CONTENT (never the app's examples-section FORMAT) lets this test
/// build the expected `fewshot_*` prompt by calling the shipped builder itself —
/// `lirevo_prompts::build_clean_system_prompt_with_examples` — so it can catch
/// drift between the generator and the app instead of eyeballing that some
/// section got appended.
fn examples() -> ExamplesByStyle {
    let mut m = HashMap::new();

    let mut casual = HashMap::new();
    casual.insert(
        "it",
        vec![
            (
                "scusa il ritardo sto arrivando tra dieci minuti".to_string(),
                "scusa il ritardo, arrivo tra 10 min".to_string(),
            ),
            (
                "ok perfetto allora ci sentiamo domani mattina".to_string(),
                "ok perfetto, ci sentiamo domani mattina".to_string(),
            ),
            (
                "no guarda oggi non ce la faccio proprio mi dispiace".to_string(),
                "no guarda oggi non ce la faccio, mi dispiace".to_string(),
            ),
        ],
    );
    casual.insert(
        "en",
        vec![
            (
                "sorry i'm late i'll be there in ten minutes".to_string(),
                "sorry i'm late, be there in 10".to_string(),
            ),
            (
                "ok great so we'll talk tomorrow morning then".to_string(),
                "ok great, talk tomorrow morning".to_string(),
            ),
            (
                "no honestly i can't make it today i'm sorry".to_string(),
                "no honestly i can't make it today, sorry".to_string(),
            ),
        ],
    );
    m.insert("casual", casual);

    let mut formal = HashMap::new();
    formal.insert(
        "it",
        vec![
            (
                "volevo chiedere se è possibile spostare la riunione a giovedì".to_string(),
                "Buongiorno,\n\nLe scrivo per chiedere se sia possibile spostare la riunione a giovedì.\n\nCordiali saluti".to_string(),
            ),
            (
                "le mando in allegato il documento che mi aveva chiesto".to_string(),
                "Buongiorno,\n\nLe invio in allegato il documento richiesto.\n\nCordiali saluti".to_string(),
            ),
            (
                "la ringrazio per la disponibilità di ieri è stato molto utile".to_string(),
                "Buongiorno,\n\nLa ringrazio per la disponibilità di ieri: l'incontro è stato molto utile.\n\nCordiali saluti".to_string(),
            ),
        ],
    );
    formal.insert(
        "en",
        vec![
            (
                "i wanted to ask whether we could move the meeting to thursday".to_string(),
                "Dear all,\n\nI am writing to ask whether we could move the meeting to Thursday.\n\nBest regards".to_string(),
            ),
            (
                "i'm attaching the document you asked me for".to_string(),
                "Dear all,\n\nPlease find attached the document you requested.\n\nBest regards".to_string(),
            ),
            (
                "thank you for your time yesterday it was really useful".to_string(),
                "Dear all,\n\nThank you for your time yesterday — the discussion was very useful.\n\nBest regards".to_string(),
            ),
        ],
    );
    m.insert("formal", formal);

    m
}

#[test]
fn style_spike_v5_corpus_and_profiles_validate() {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let corpus_path = here.join("data/corpus/style-spike-v5.jsonl");
    let profiles_path = here.join("data/profiles/style-spike-v5.toml");

    let cases = lirevo_eval::corpus::load_jsonl(&corpus_path).expect("load style-spike-v5 corpus");
    let profiles =
        lirevo_eval::profiles::load_toml(&profiles_path).expect("load style-spike-v5 profiles");
    lirevo_eval::profiles::validate(&cases, &profiles).expect("cross-ref");

    // Every profile must exist for every language under test.
    let mut seen = HashSet::new();
    for c in &cases {
        seen.insert((c.profile.clone(), c.language.clone()));
    }
    for profile in &["baseline", "fewshot_casual", "fewshot_formal"] {
        for lang in &["it", "en"] {
            assert!(
                seen.contains(&((*profile).to_string(), (*lang).to_string())),
                "missing coverage: profile={profile} lang={lang}",
            );
        }
    }

    // The anti-drift guard: `baseline` must be the real shipped prompt,
    // byte-identical to `build_clean_system_prompt`, for both languages.
    for lang in &["it", "en"] {
        let shipped = lirevo_prompts::build_clean_system_prompt(lang);
        let baseline = &profiles["baseline"].system_prompts[*lang];
        assert_eq!(
            baseline, &shipped,
            "baseline[{lang}] has drifted from lirevo_prompts::build_clean_system_prompt",
        );
    }

    // The core anti-drift guard for v5: `fewshot_*` must be byte-identical to
    // the SHIPPED few-shot builder called with this test's own copy of the
    // examples. This proves the generator actually went through
    // `build_clean_system_prompt_with_examples` (via the subcommand) rather
    // than hand-formatting an examples section that merely looks similar.
    let ex = examples();
    for style in &["casual", "formal"] {
        let profile_name = format!("fewshot_{style}");
        let style_examples = ex
            .get(*style)
            .expect("style present in test's EXAMPLES mirror");
        for lang in &["it", "en"] {
            let pairs = style_examples
                .get(*lang)
                .expect("lang present in test's EXAMPLES mirror");
            let shipped_baseline = lirevo_prompts::build_clean_system_prompt(lang);
            let shipped_fewshot =
                lirevo_prompts::build_clean_system_prompt_with_examples(lang, pairs);
            let prompt = &profiles[profile_name.as_str()].system_prompts[*lang];

            assert_eq!(
                prompt, &shipped_fewshot,
                "{profile_name}[{lang}] has drifted from build_clean_system_prompt_with_examples",
            );
            assert!(
                prompt.starts_with(&shipped_baseline),
                "{profile_name}[{lang}] does not start with the shipped baseline prompt",
            );
        }
    }

    // Leakage guard: no test case's `transcript` may appear as an example's
    // raw side, and no `expected` may appear as an example's final side —
    // compared trimmed and lowercased. If a test transcript leaked into the
    // examples, the model would copy the pinned answer and the run would
    // report a triumph that measured nothing.
    let example_raws: HashSet<String> = ex
        .values()
        .flat_map(HashMap::values)
        .flatten()
        .map(|(raw, _)| raw.trim().to_lowercase())
        .collect();
    let example_finals: HashSet<String> = ex
        .values()
        .flat_map(HashMap::values)
        .flatten()
        .map(|(_, final_text)| final_text.trim().to_lowercase())
        .collect();
    for c in &cases {
        let transcript = c.transcript.trim().to_lowercase();
        let expected = c.expected.trim().to_lowercase();
        assert!(
            !example_raws.contains(&transcript),
            "case {} transcript leaks into a few-shot example's raw side",
            c.id,
        );
        assert!(
            !example_finals.contains(&expected),
            "case {} expected leaks into a few-shot example's final side",
            c.id,
        );
    }

    // The experiment is only readable if each source case appears in both
    // arms with a byte-identical `expected`, and the fewshot arm carries the
    // style of its OWN source (a casual case scored against a formal card
    // would report "fewshot fails" from an authoring error, not the model).
    let mut by_stem: HashMap<String, Vec<&lirevo_eval::corpus::TestCase>> = HashMap::new();
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
        assert_eq!(group.len(), 2, "stem {stem} must have exactly 2 arms");

        let expected = &group[0].expected;
        for c in group {
            assert_eq!(
                &c.expected, expected,
                "arms of {stem} disagree on `expected`; they would not be comparable",
            );
        }

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
                "fewshot_casual"
            } else {
                "fewshot_formal"
            },
        ]);
        assert_eq!(
            arms, want,
            "stem {stem} ({style}) has the wrong arm profiles"
        );
    }
}
