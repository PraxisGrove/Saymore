use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use ort::{
    session::{Session, builder::GraphOptimizationLevel},
    value::{Tensor, TensorElementType, ValueType},
};
use template_app::SpeechRecognitionError;

use super::{
    SeacoDecoder,
    frontend::{FEATURE_DIM, Frontend},
    lock_error,
    model::ModelFiles,
    protocol,
    vocabulary::{EMBEDDING_DIM, Hotwords, VOCAB_SIZE, Vocabulary, WORD_WIDTH},
};

mod initialization;

pub(super) struct OrtDecoder {
    model: Mutex<Session>,
    embedding: Mutex<Session>,
    frontend: Frontend,
    vocabulary: Arc<Vocabulary>,
}

impl OrtDecoder {
    pub fn load(
        files: &ModelFiles,
        frontend: Frontend,
        vocabulary: Arc<Vocabulary>,
    ) -> Result<Self, SpeechRecognitionError> {
        initialization::initialize()?;
        let embedding = load_session(&files.embedding)?;
        let model = load_session(&files.model)?;
        check_contract(&model, &embedding)?;
        Ok(Self {
            model: Mutex::new(model),
            embedding: Mutex::new(embedding),
            frontend,
            vocabulary,
        })
    }
}

impl SeacoDecoder for OrtDecoder {
    fn embed(&self, hotwords: &Hotwords) -> Result<Vec<f32>, SpeechRecognitionError> {
        let count = hotwords.lengths.len();
        let input = Tensor::from_array(([count, WORD_WIDTH], hotwords.matrix.clone()))
            .map_err(runtime_error)?;
        let mut session = self.embedding.lock().map_err(lock_error)?;
        let outputs = session
            .run(ort::inputs!["hotword" => input])
            .map_err(runtime_error)?;
        let output = outputs
            .get("hw_embed")
            .ok_or_else(|| protocol("embedding output is missing"))?;
        let (shape, values) = output.try_extract_tensor::<f32>().map_err(runtime_error)?;
        if shape.as_ref() != [WORD_WIDTH as i64, count as i64, EMBEDDING_DIM as i64]
            || values.iter().any(|value| !value.is_finite())
        {
            return Err(protocol(
                "embedding output has invalid dimensions or values",
            ));
        }
        let mut bias = Vec::with_capacity(count * EMBEDDING_DIM);
        for (word, &length) in hotwords.lengths.iter().enumerate() {
            if !(1..=WORD_WIDTH).contains(&length) {
                return Err(protocol("hotword token length is invalid"));
            }
            // ONNX returns [token, word, channel]. Select each word's last valid token.
            let start = ((length - 1) * count + word) * EMBEDDING_DIM;
            bias.extend_from_slice(&values[start..start + EMBEDDING_DIM]);
        }
        Ok(bias)
    }

    fn decode(&self, samples: &[i16], bias: &[f32]) -> Result<String, SpeechRecognitionError> {
        if samples.iter().all(|sample| *sample == 0) {
            return Ok(String::new());
        }
        if bias.is_empty() || !bias.len().is_multiple_of(EMBEDDING_DIM) {
            return Err(protocol("bias embedding dimensions are invalid"));
        }
        let features = self.frontend.features(samples)?;
        let frames = features.len() / FEATURE_DIM;
        let speech =
            Tensor::from_array(([1, frames, FEATURE_DIM], features)).map_err(runtime_error)?;
        let lengths = Tensor::from_array(([1], vec![frames as i32])).map_err(runtime_error)?;
        let bias = Tensor::from_array((
            [1, bias.len() / EMBEDDING_DIM, EMBEDDING_DIM],
            bias.to_vec(),
        ))
        .map_err(runtime_error)?;
        let mut session = self.model.lock().map_err(lock_error)?;
        let outputs = session
            .run(
                ort::inputs!["speech" => speech, "speech_lengths" => lengths, "bias_embed" => bias],
            )
            .map_err(runtime_error)?;
        let logits = outputs
            .get("logits")
            .ok_or_else(|| protocol("logits output is missing"))?;
        let (shape, logits) = logits.try_extract_tensor::<f32>().map_err(runtime_error)?;
        let token_num = outputs
            .get("token_num")
            .ok_or_else(|| protocol("token count output is missing"))?;
        let (count_shape, count) = token_num
            .try_extract_tensor::<i32>()
            .map_err(runtime_error)?;
        if shape.len() != 3
            || shape[0] != 1
            || shape[2] != VOCAB_SIZE as i64
            || count_shape.as_ref() != [1]
            || count.len() != 1
            || count[0] < 0
            || i64::from(count[0]) > shape[1]
            || logits.iter().any(|value| !value.is_finite())
        {
            return Err(protocol(
                "decoder outputs have invalid dimensions or values",
            ));
        }
        let mut ids = Vec::with_capacity(count[0] as usize);
        for row in logits
            .as_chunks::<VOCAB_SIZE>()
            .0
            .iter()
            .take(count[0] as usize)
        {
            let id = row
                .iter()
                .enumerate()
                .max_by(|(_, left), (_, right)| left.total_cmp(right))
                .map(|(id, _)| id)
                .ok_or_else(|| protocol("decoder output row is empty"))?;
            ids.push(id);
        }
        self.vocabulary.text(&ids)
    }
}

fn load_session(path: &Path) -> Result<Session, SpeechRecognitionError> {
    (|| -> ort::Result<Session> {
        Session::builder()?
            .with_intra_threads(4)?
            .with_inter_threads(1)?
            .with_optimization_level(GraphOptimizationLevel::Level1)?
            .with_logger(Arc::new(|_, _, _, _, _| {}))?
            .commit_from_file(path)
    })()
    .map_err(runtime_error)
}

fn check_contract(model: &Session, embedding: &Session) -> Result<(), SpeechRecognitionError> {
    use TensorElementType::{Float32, Int32};
    if model.inputs().len() != 3 || embedding.inputs().len() != 1 {
        return Err(protocol("graphs have an unsupported input contract"));
    }
    for (session, name, ty, shape) in [
        (model, "speech", Float32, &[-1, -1, 560][..]),
        (model, "speech_lengths", Int32, &[-1][..]),
        (model, "bias_embed", Float32, &[-1, -1, 512][..]),
        (embedding, "hotword", Int32, &[-1, 10][..]),
    ] {
        check_tensor(
            session
                .inputs()
                .iter()
                .find(|input| input.name() == name)
                .map(|input| input.dtype()),
            ty,
            shape,
        )?;
    }
    for (session, name, ty, shape) in [
        (model, "logits", Float32, &[-1, -1, 8404][..]),
        (model, "token_num", Int32, &[-1][..]),
        (embedding, "hw_embed", Float32, &[10, -1, 512][..]),
    ] {
        check_tensor(
            session
                .outputs()
                .iter()
                .find(|output| output.name() == name)
                .map(|output| output.dtype()),
            ty,
            shape,
        )?;
    }
    Ok(())
}

fn check_tensor(
    value: Option<&ValueType>,
    ty: TensorElementType,
    expected: &[i64],
) -> Result<(), SpeechRecognitionError> {
    match value {
        Some(ValueType::Tensor {
            ty: actual, shape, ..
        }) if *actual == ty
            && shape.len() == expected.len()
            && shape
                .iter()
                .zip(expected)
                .all(|(actual, expected)| *expected == -1 || actual == expected) =>
        {
            Ok(())
        }
        _ => Err(protocol(
            "graph tensor names, types or dimensions are unsupported",
        )),
    }
}

fn runtime_error(_error: ort::Error) -> SpeechRecognitionError {
    // ORT errors can embed user paths. Keep the ownership-boundary error content-free.
    protocol("native CPU inference failed")
}
