use super::{
    ModelArtifact, ModelManifest, SENSE_VOICE_MODEL_ID, SENSE_VOICE_MODEL_REVISION, artifact,
};

const BASE_URL: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/2365baeacb507f821a0c8120fcee3d484dba7a07";

impl ModelManifest {
    pub(super) fn sense_voice_small() -> Self {
        Self::direct(
            SENSE_VOICE_MODEL_ID,
            SENSE_VOICE_MODEL_REVISION,
            BASE_URL,
            vec![
                artifact(
                    "model.int8.onnx",
                    239_233_841,
                    "c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51",
                ),
                artifact(
                    "tokens.txt",
                    315_894,
                    "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc",
                ),
                artifact(
                    "LICENSE",
                    71,
                    "221c6df10b0931a5629adad671ea48fb7747e034c414b6d2bfa275bc3dd4ea17",
                ),
                ModelArtifact {
                    remote_path: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx".to_owned(),
                    local_path: "silero_vad.onnx".to_owned(),
                    bytes: 643_854,
                    sha256: "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6".to_owned(),
                },
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_pinned_to_the_verified_int8_release() {
        let manifest = ModelManifest::sense_voice_small();

        assert_eq!(SENSE_VOICE_MODEL_ID, manifest.id);
        assert_eq!(SENSE_VOICE_MODEL_REVISION, manifest.revision);
        assert_eq!(240_193_660, manifest.total_bytes());
        assert_eq!(4, manifest.artifacts.len());
    }
}
