//! Print the shipped cleanup system prompt for a language, so eval data can be
//! generated from the real prompt instead of a hand-transcribed copy.

use anyhow::Result;

use super::PrintShippedPromptArgs;

pub fn run(args: &PrintShippedPromptArgs) -> Result<()> {
    print!(
        "{}",
        lirevo_prompts::build_clean_system_prompt(&args.language)
    );
    Ok(())
}
