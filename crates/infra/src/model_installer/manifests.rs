use super::*;

const QWEN3_ASR_BASE_URL: &str = "https://huggingface.co/sakasegawa/Qwen3-ASR-1.7B-GGUF/resolve/81e2423d24073cdeeccb5951df16accc89e075a2";
const PUNCTUATION_BASE_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/punctuation-models";
const PUNCTUATION_PACKAGE: &str =
    "sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8.tar.bz2";
const PUNCTUATION_PACKAGE_DIRECTORY: &str =
    "sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8";

impl ModelManifest {
    pub(super) fn whisper_large_v3_turbo() -> Self {
        Self::direct(
            WHISPER_MODEL_ID,
            WHISPER_MODEL_REVISION,
            &format!(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/{WHISPER_MODEL_REVISION}"
            ),
            vec![artifact(
                WHISPER_MODEL_FILE,
                WHISPER_MODEL_BYTES,
                "317eb69c11673c9de1e1f0d459b253999804ec71ac4c23c17ecf5fbe24e259a1",
            )],
        )
    }

    pub(super) fn qwen3_asr_1_7b() -> Self {
        Self::direct(
            QWEN3_ASR_MODEL_ID,
            QWEN3_ASR_MODEL_REVISION,
            QWEN3_ASR_BASE_URL,
            vec![artifact(
                QWEN3_ASR_MODEL_FILE,
                QWEN3_ASR_MODEL_BYTES,
                "5f219b78a1d9c3b9e97da27708b36f8a0bc1bfc1650b541c0a6dbaf87c9a62d0",
            )],
        )
    }

    pub(super) fn punctuation() -> Self {
        let archive = artifact(
            PUNCTUATION_PACKAGE,
            64_717_756,
            "c0d5aa5f8eeb686032345e180bedf39319dc2e0556781c6264bcadba8328a6e1",
        );
        Self {
            id: PUNCTUATION_MODEL_ID.to_owned(),
            revision: PUNCTUATION_MODEL_REVISION.to_owned(),
            base_url: PUNCTUATION_BASE_URL.to_owned(),
            downloads: vec![archive],
            artifacts: vec![artifact(
                "model.int8.onnx",
                75_519_198,
                "65a3fb9f5ad7bfb96bf69e0dc4481df97f6ee60513c1d94ce981ba6effd524b1",
            )],
            preparation: ModelPreparation::TarBzip2 {
                archive_local_path: PUNCTUATION_PACKAGE.to_owned(),
                member_path: format!("{PUNCTUATION_PACKAGE_DIRECTORY}/model.int8.onnx"),
            },
        }
    }
}
