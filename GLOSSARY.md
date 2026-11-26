# Saymore

Saymore turns speech into text for dictation.

## Language

**Text polishing**: The optional treatment of a recognized transcript for
readability. It includes local speech cleanup and can use an LLM for further
refinement; it does not include ordinary whitespace or dictionary spelling
normalization.

**Local speech cleanup**: The part of text polishing that removes clear fillers
and accidental repetitions without sending the transcript to a provider.
