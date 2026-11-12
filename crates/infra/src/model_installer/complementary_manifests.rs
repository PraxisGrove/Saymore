use super::*;

impl ModelManifest {
    pub(super) fn zipformer() -> Self {
        Self::direct(
            ZIPFORMER_MODEL_ID,
            ZIPFORMER_MODEL_REVISION,
            &format!(
                "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/resolve/{ZIPFORMER_MODEL_REVISION}"
            ),
            vec![
                artifact(
                    "encoder-epoch-99-avg-1.int8.onnx",
                    181_895_032,
                    "8fa764187a261844f859d7143ebaa563af5d10adfece4c18a8f414c88cba2a9b",
                ),
                artifact(
                    "decoder-epoch-99-avg-1.onnx",
                    13_876_452,
                    "2e3b5ec371f8899ee6acd829fd753ba45772df57a91bdf37cde3136354e7db7d",
                ),
                artifact(
                    "joiner-epoch-99-avg-1.onnx",
                    12_833_618,
                    "5f2adc585dd1bec6421c8bb8660d2a73fc8b9ceb24491ef51399ba2a2f0fc31b",
                ),
                artifact(
                    "tokens.txt",
                    56_317,
                    "a8e0e4ec53810e433789b54a5c0134a7eaa2ffca595a6334d54c00da858841d3",
                ),
                artifact(
                    "bpe.vocab",
                    12_564,
                    "d0b642f3a2eacd5fadefdeff9e0e1358cab729647cbb7fe58cf738e1f7407029",
                ),
                artifact(
                    "README.md",
                    296,
                    "b6f9458f4208ae821beaaf11dc983486916a4089a2f3677b40f9ff06ec4e6440",
                ),
            ],
        )
    }
}
