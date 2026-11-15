use super::*;

pub const SEACO_MODEL_ID: &str = "seaco-paraformer-zh-int8";
pub const SEACO_MODEL_REVISION: &str = "45796913d5696c6334d8e5e18fc98cf4d063ad88";

impl ModelManifest {
    pub(super) fn seaco_paraformer() -> Self {
        Self::direct(
            SEACO_MODEL_ID,
            SEACO_MODEL_REVISION,
            &format!(
                "https://huggingface.co/MoYoYoTech/speech_seaco_paraformer_large_asr_nat-zh-cn-16k-common-vocab8404-pytorch/resolve/{SEACO_MODEL_REVISION}"
            ),
            vec![
                artifact(
                    "model_quant.onnx",
                    345_139_149,
                    "1382593b11a18e0872ee149542496cbf6d4881aaeaba93a3de97bfa1cb5e7626",
                ),
                artifact(
                    "model_eb_quant.onnx",
                    34_028_133,
                    "967e4f4f82706df80efb72a3b9f9ecc6e5d4e46ddb11eaf6837ae17ec4e04b06",
                ),
                artifact(
                    "am.mvn",
                    11_203,
                    "29b3c740a2c0cfc6b308126d31d7f265fa2be74f3bb095cd2f143ea970896ae5",
                ),
                artifact(
                    "config.yaml",
                    3_477,
                    "6602efa95e4c7248e1d1030f7ff454a3c9af0f57c335ed87f35260cd7faec35d",
                ),
                artifact(
                    "tokens.json",
                    93_676,
                    "2b20c2b12572d682afff84ce1c8d560f67b8b32a4c1f21567411d141ed352127",
                ),
                artifact(
                    "seg_dict",
                    8_287_834,
                    "59a2ef803a3f1648ad03a2e1480db1c1ee0c0d7dc4ef4dbd16cea33944329022",
                ),
            ],
        )
    }
}
