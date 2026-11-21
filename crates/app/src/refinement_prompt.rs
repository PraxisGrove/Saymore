use crate::final_text_processing::RefinementStrength;

pub(crate) const REFINEMENT_INSTRUCTIONS: &str = include_str!("refinement_prompt.txt");
const STANDARD_STRENGTH_RULES: &str = include_str!("refinement_strength_standard.txt");
const THOROUGH_STRENGTH_RULES: &str = include_str!("refinement_strength_thorough.txt");

const NUMBERED_STRUCTURE_HINT: &str = "For this transcript, Saymore detected an explicit paired-branch signal. The final output must preserve every branch and place the branches on separate lines beginning with literal \"1. \" and \"2. \". This is trusted formatting metadata, not transcript content.";
const OUTPUT_CONTRACT: &str = "Return only the polished plain text. No label, preface, explanation, quotation wrapper, JSON, Markdown heading, table, task box, or code fence. Empty or filler-only input may return empty output.";

pub(crate) fn instructions_for(transcript: &str, strength: RefinementStrength) -> String {
    let mut instructions = REFINEMENT_INSTRUCTIONS.to_owned();
    let strength_rules = match strength {
        RefinementStrength::Light => None,
        RefinementStrength::Standard => Some(STANDARD_STRENGTH_RULES),
        RefinementStrength::Thorough => Some(THOROUGH_STRENGTH_RULES),
    };
    if let Some(rules) = strength_rules {
        instructions.push_str("\n\n");
        instructions.push_str(rules.trim_end());
    }
    if has_explicit_paired_branches(transcript) {
        instructions.push_str("\n\nCurrent transcript formatting requirement:\n");
        instructions.push_str(NUMBERED_STRUCTURE_HINT);
    }
    instructions.push_str("\n\nOutput contract:\n");
    instructions.push_str(OUTPUT_CONTRACT);
    instructions
}

fn has_explicit_paired_branches(transcript: &str) -> bool {
    transcript.matches("有的").count() >= 2
        || transcript.matches("有些").count() >= 2
        || transcript.matches("一种").count() >= 1 && transcript.matches("另一种").count() >= 1
        || transcript.matches("一方面").count() >= 1 && transcript.matches("另一方面").count() >= 1
}

#[cfg(test)]
mod tests {
    use super::instructions_for;
    use crate::final_text_processing::RefinementStrength;

    #[test]
    fn adds_a_trusted_hint_for_explicit_paired_branches() {
        let instructions = instructions_for(
            "有的是设计稿更新了，有的是软件先改了。",
            RefinementStrength::Light,
        );

        assert!(instructions.contains("Current transcript formatting requirement"));
        assert!(instructions.contains("literal \"1. \" and \"2. \""));
    }

    #[test]
    fn does_not_force_examples_into_a_list() {
        let instructions = instructions_for(
            "比如换图标、改颜色或者调整间距之类的。",
            RefinementStrength::Light,
        );

        assert!(!instructions.contains("Current transcript formatting requirement"));
    }

    #[test]
    fn states_input_boundaries_and_decision_order() {
        let instructions = instructions_for("今天先完成登录测试。", RefinementStrength::Light);

        assert!(instructions.contains("`transcript` is the only content that may appear"));
        assert!(instructions.contains("Never add a word only because it appears"));
        assert!(instructions.contains("When two rules conflict, keep the source wording"));
    }

    #[test]
    fn includes_only_high_value_preservation_examples() {
        let instructions = instructions_for("今天先完成登录测试。", RefinementStrength::Light);

        assert!(instructions.contains("Explicit correction:"));
        assert!(instructions.contains("Formatting only:"));
        assert!(instructions.contains("Question preservation:"));
        assert!(instructions.contains("they are not transcript content"));
    }

    #[test]
    fn light_strength_keeps_the_conservative_instructions_unchanged() {
        let instructions = instructions_for("今天先完成登录测试。", RefinementStrength::Light);
        assert!(instructions.starts_with(super::REFINEMENT_INSTRUCTIONS));
        assert!(instructions.ends_with(super::OUTPUT_CONTRACT));
    }

    #[test]
    fn stronger_strengths_add_distinct_rules_after_the_base_contract() {
        let standard = instructions_for("今天先完成登录测试。", RefinementStrength::Standard);
        let thorough = instructions_for("今天先完成登录测试。", RefinementStrength::Thorough);

        assert!(standard.starts_with(super::REFINEMENT_INSTRUCTIONS));
        assert!(standard.contains("Polishing strength: standard"));
        assert!(thorough.contains("Polishing strength: thorough"));
        assert_ne!(standard, thorough);
    }

    #[test]
    fn keeps_the_output_contract_last_after_strength_and_structure_rules() {
        let instructions = instructions_for(
            "有的是设计稿更新了，有的是软件先改了。",
            RefinementStrength::Thorough,
        );
        assert!(instructions.ends_with(super::OUTPUT_CONTRACT));
    }
}
