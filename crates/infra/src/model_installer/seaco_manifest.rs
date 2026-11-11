use super::*;

pub const SEACO_MODEL_ID: &str = "seaco-paraformer-zh-fp32";
pub const SEACO_MODEL_REVISION: &str = "0141367fdc9b6ba58b0442ef34bceb56a6c1789c-onnx-1.2.6";

impl ModelManifest {
    pub(super) fn seaco_paraformer() -> Self {
        Self {
            id: SEACO_MODEL_ID.to_owned(),
            revision: SEACO_MODEL_REVISION.to_owned(),
            base_url: String::new(),
            downloads: Vec::new(),
            artifacts: vec![
                artifact(
                    "model.onnx",
                    957_692_158,
                    "20280dab4185fd35556f021e5c8d343d40e107bce1701afaa1e24654cc9f3dee",
                ),
                artifact(
                    "model_eb.onnx",
                    34_026_072,
                    "37fc62cb078a341617af90aca5a753e71c3acaee60cdabbc2c19000d45cfd938",
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
            preparation: ModelPreparation::LocalImport,
        }
    }
}
