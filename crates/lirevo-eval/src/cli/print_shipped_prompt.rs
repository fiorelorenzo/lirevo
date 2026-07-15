//! Print the shipped cleanup system prompt for a language, so eval data can be
//! generated from the real prompt instead of a hand-transcribed copy.

use anyhow::{Context, Result};

use super::PrintShippedPromptArgs;

pub fn run(args: &PrintShippedPromptArgs) -> Result<()> {
    let prompt = match &args.examples_json {
        Some(json) => {
            let examples: Vec<(String, String)> =
                serde_json::from_str(json).with_context(|| {
                    format!(
                        "--examples-json is not a valid JSON array of [raw, final] pairs: {json}"
                    )
                })?;
            lirevo_prompts::build_clean_system_prompt_with_examples(&args.language, &examples)
        }
        None => lirevo_prompts::build_clean_system_prompt(&args.language),
    };
    print!("{prompt}");
    Ok(())
}
