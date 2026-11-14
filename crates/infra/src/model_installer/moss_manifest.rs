use super::*;

pub const MOSS_MODEL_ID: &str = "moss-transcribe-diarize-fp16";
pub const MOSS_MODEL_REVISION: &str = "3110ffbd08d94d15bdf931c636284f042d21e3cc";
pub(crate) const MOSS_FILES: [(&str, u64, &str); 3] = [
    (
        "audio_encoder.onnx",
        625_165_991,
        "07341e7e7b4271d34a4023e3dc204ec3912cfaa8840778a0827d06d6e130eb88",
    ),
    (
        "decoder.onnx",
        1_504_146_682,
        "a00c0dde2828a6f6ead46531dee9a2c34e32afa67ed4e46042282a65dea6c447",
    ),
    (
        "tokenizer.json",
        11_423_222,
        "bcf03774334462d6e34b5005cb11120a62275f146ee2953e68731ecdbce84fbb",
    ),
];

impl ModelManifest {
    pub(super) fn moss() -> Self {
        Self::direct(
            MOSS_MODEL_ID,
            MOSS_MODEL_REVISION,
            &format!(
                "https://huggingface.co/soniqo/MOSS-Transcribe-Diarize-0.9B-ONNX-FP16/resolve/{MOSS_MODEL_REVISION}"
            ),
            MOSS_FILES
                .iter()
                .map(|(name, bytes, hash)| artifact(name, *bytes, hash))
                .collect(),
        )
    }
}
