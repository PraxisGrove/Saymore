pub(crate) const REFINEMENT_INSTRUCTIONS: &str = include_str!("refinement_prompt.txt");

const NUMBERED_STRUCTURE_HINT: &str = "For this transcript, Saymore detected an explicit paired-branch signal. The final output must preserve every branch and place the branches on separate lines beginning with literal \"1. \" and \"2. \". This is trusted formatting metadata, not transcript content.";

pub(crate) fn instructions_for(transcript: &str) -> String {
    let mut instructions = REFINEMENT_INSTRUCTIONS.to_owned();
    if has_explicit_paired_branches(transcript) {
        instructions.push_str("\n\nCurrent transcript formatting requirement:\n");
        instructions.push_str(NUMBERED_STRUCTURE_HINT);
    }
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

    #[test]
    fn adds_a_trusted_hint_for_explicit_paired_branches() {
        let instructions = instructions_for("有的是设计稿更新了，有的是软件先改了。");

        assert!(instructions.contains("Current transcript formatting requirement"));
        assert!(instructions.contains("literal \"1. \" and \"2. \""));
    }

    #[test]
    fn does_not_force_examples_into_a_list() {
        let instructions = instructions_for("比如换图标、改颜色或者调整间距之类的。");

        assert!(!instructions.contains("Current transcript formatting requirement"));
    }
}
