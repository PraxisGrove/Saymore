use super::{MossDecoder, frontend::Frontend, lock_error, prompt, protocol};
use half::f16;
use ort::{
    session::{Session, builder::GraphOptimizationLevel},
    value::{Tensor, TensorRef},
};
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
};
use template_app::SpeechRecognitionError;
use tokenizers::Tokenizer;

mod cache;
mod contract;
use cache::Cache;

pub(super) struct OrtDecoder {
    sessions: Mutex<Sessions>,
    frontend: Frontend,
    tokenizer: Tokenizer,
}

struct Sessions {
    audio: Session,
    decoder: Session,
}

impl OrtDecoder {
    pub fn load(directory: &Path) -> Result<Self, SpeechRecognitionError> {
        for (name, bytes, _) in crate::model_installer::MOSS_FILES {
            let metadata = fs::metadata(directory.join(name))
                .map_err(|_| protocol("model files are missing"))?;
            if !metadata.is_file() || metadata.len() != bytes {
                return Err(protocol("model files do not match the pinned artifact"));
            }
        }
        crate::ort_runtime::initialize()?;
        let tokenizer = Tokenizer::from_file(directory.join("tokenizer.json"))
            .map_err(|_| protocol("tokenizer could not be loaded"))?;
        let audio = load_session(&directory.join("audio_encoder.onnx"), false)?;
        let decoder = load_session(&directory.join("decoder.onnx"), true)?;
        contract::check(&audio, &decoder)?;
        Ok(Self {
            sessions: Mutex::new(Sessions { audio, decoder }),
            frontend: Frontend::new(),
            tokenizer,
        })
    }
}

impl MossDecoder for OrtDecoder {
    fn decode(&self, samples: &[i16], hints: &str) -> Result<String, SpeechRecognitionError> {
        let features: Vec<_> = self
            .frontend
            .features(samples)?
            .into_iter()
            .map(f16::from_f32)
            .collect();
        let mut sessions = self.sessions.lock().map_err(lock_error)?;
        let embeddings = encode_audio(&mut sessions.audio, features, samples.len().div_ceil(1280))?;
        let mut input = prompt::build(&self.tokenizer, embeddings.len() / 1024, hints)?;
        let mut cache = Cache::default();
        let mut generated = Vec::new();
        let limit = samples.len().div_ceil(1280) + 128;
        for _ in 0..limit {
            let token = decode_step(&mut sessions.decoder, &input, &embeddings, &mut cache)?;
            if matches!(token, 151643 | 151645) {
                return self
                    .tokenizer
                    .decode(&generated, true)
                    .map_err(|_| protocol("could not decode generated tokens"));
            }
            generated.push(token);
            input = vec![i64::from(token)];
        }
        Err(protocol("generation exceeded its token limit"))
    }
}

fn load_session(path: &Path, prepacking: bool) -> Result<Session, SpeechRecognitionError> {
    (|| -> ort::Result<Session> {
        Session::builder()?
            .with_intra_threads(4)?
            .with_inter_threads(1)?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_prepacking(prepacking)?
            .with_logger(Arc::new(|_, _, _, _, _| {}))?
            .commit_from_file(path)
    })()
    .map_err(runtime_error)
}

fn encode_audio(
    session: &mut Session,
    features: Vec<f16>,
    frames: usize,
) -> Result<Vec<f16>, SpeechRecognitionError> {
    let input = Tensor::from_array(([1, 80, 3000], features)).map_err(runtime_error)?;
    let outputs = session
        .run(ort::inputs!["input_features" => input])
        .map_err(runtime_error)?;
    let (shape, data) = outputs
        .get("audio_embeds")
        .ok_or_else(|| protocol("audio output is missing"))?
        .try_extract_tensor::<f16>()
        .map_err(runtime_error)?;
    if shape.as_ref() != [1, 375, 1024]
        || frames > 375
        || data.iter().any(|value| !value.is_finite())
    {
        return Err(protocol("audio embeddings are invalid"));
    }
    Ok(data[..frames * 1024].to_vec())
}

fn decode_step(
    session: &mut Session,
    ids: &[i64],
    audio: &[f16],
    cache: &mut Cache,
) -> Result<u32, SpeechRecognitionError> {
    let count = ids.len();
    let past = cache.length;
    let mut embeds = vec![f16::ZERO; count * 1024];
    let mut embed_mask = vec![f16::ZERO; count];
    let mut cursor = 0;
    for (index, token) in ids.iter().enumerate() {
        if *token == prompt::AUDIO_TOKEN {
            let frame = audio
                .get(cursor..cursor + 1024)
                .ok_or_else(|| protocol("audio placeholder count is invalid"))?;
            embeds[index * 1024..(index + 1) * 1024].copy_from_slice(frame);
            embed_mask[index] = f16::ONE;
            cursor += 1024;
        }
    }
    let positions: Vec<i64> = (past..past + count).map(|i| i as i64).collect();
    let mut mask = vec![f16::NEG_INFINITY; count * (past + count)];
    for query in 0..count {
        mask[query * (past + count)..query * (past + count) + past + query + 1].fill(f16::ZERO);
    }
    let outputs = session.run(ort::inputs![
        "input_ids" => TensorRef::from_array_view(([1, count], ids)).map_err(runtime_error)?,
        "input_embeds" => Tensor::from_array(([1, count, 1024], embeds)).map_err(runtime_error)?,
        "input_embed_mask" => Tensor::from_array(([1, count, 1], embed_mask)).map_err(runtime_error)?,
        "position_ids" => Tensor::from_array(([count], positions)).map_err(runtime_error)?,
        "attention_mask" => Tensor::from_array(([1, 1, count, past + count], mask)).map_err(runtime_error)?,
        "past_keys" => cache_tensor(&cache.keys, past)?,
        "past_values" => cache_tensor(&cache.values, past)?,
    ]).map_err(runtime_error)?;
    let (shape, logits) = outputs
        .get("logits")
        .ok_or_else(|| protocol("logits are missing"))?
        .try_extract_tensor::<f16>()
        .map_err(runtime_error)?;
    if shape.as_ref() != [1, 1, 151936] || logits.iter().any(|value| !value.is_finite()) {
        return Err(protocol("decoder logits are invalid"));
    }
    let token = logits
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.to_f32().total_cmp(&b.to_f32()))
        .map(|(index, _)| index as u32)
        .ok_or_else(|| protocol("decoder logits are empty"))?;
    let (key_shape, keys) = outputs
        .get("new_keys")
        .ok_or_else(|| protocol("key cache is missing"))?
        .try_extract_tensor::<f16>()
        .map_err(runtime_error)?;
    let (value_shape, values) = outputs
        .get("new_values")
        .ok_or_else(|| protocol("value cache is missing"))?
        .try_extract_tensor::<f16>()
        .map_err(runtime_error)?;
    let expected = [28, 1, 8, count as i64, 128];
    if key_shape.as_ref() != expected
        || value_shape.as_ref() != expected
        || keys.iter().chain(values).any(|value| !value.is_finite())
    {
        return Err(protocol("decoder cache is invalid"));
    }
    cache.append(count, keys, values)?;
    Ok(token)
}

fn cache_tensor(data: &[f16], past: usize) -> Result<Tensor<f16>, SpeechRecognitionError> {
    // ORT permits an empty KV axis, but ort's raw-data constructor rejects it.
    // Use the allocator constructor for the zero-element initial cache.
    if past == 0 {
        Tensor::new(&ort::memory::Allocator::default(), [28usize, 1, 8, 0, 128])
            .map_err(runtime_error)
    } else {
        Tensor::from_array(([28, 1, 8, past, 128], data.to_vec())).map_err(runtime_error)
    }
}

fn runtime_error(_error: ort::Error) -> SpeechRecognitionError {
    protocol("native inference failed")
}
