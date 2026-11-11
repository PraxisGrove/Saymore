use super::*;

const QWEN3_ASR_BASE_URL: &str = "https://www.modelscope.cn/models/zengshuishui/Qwen3-ASR-onnx/resolve/cb045ad80b8970c9d411d463e5b78991a566596c";
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
            vec![
                artifact_at(
                    "model_1.7B/conv_frontend.onnx",
                    "conv_frontend.onnx",
                    48_080_441,
                    "fa894a4ba53da6a4238f2a6ca0b09362e505d39cecbd646051b033e2e8d7e2fb",
                ),
                artifact_at(
                    "model_1.7B/encoder.int8.onnx",
                    "encoder.int8.onnx",
                    314_222_162,
                    "436fbd910a0c8914851e5ac1354e807be9f283d08a5da728adaa609731c41469",
                ),
                artifact_at(
                    "model_1.7B/decoder.int8.onnx",
                    "decoder.int8.onnx",
                    2_037_458_645,
                    "c43c853fa6e97d08365cb8a5502b360b595cd43c00dc60e4d8ca7cc18cad460b",
                ),
                artifact_at(
                    "tokenizer/merges.txt",
                    "tokenizer/merges.txt",
                    1_671_853,
                    "8831e4f1a044471340f7c0a83d7bd71306a5b867e95fd870f74d0c5308a904d5",
                ),
                artifact_at(
                    "tokenizer/tokenizer_config.json",
                    "tokenizer/tokenizer_config.json",
                    12_487,
                    "4942d005604266809309cabc9f4e9cb89ce855d59b14681fdc0e1cc62ea26c4c",
                ),
                artifact_at(
                    "tokenizer/vocab.json",
                    "tokenizer/vocab.json",
                    2_776_833,
                    "ca10d7e9fb3ed18575dd1e277a2579c16d108e32f27439684afa0e10b1440910",
                ),
            ],
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
