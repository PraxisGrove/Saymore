use super::super::protocol;
use ort::{
    session::Session,
    value::{TensorElementType, ValueType},
};
use template_app::SpeechRecognitionError;

pub(super) fn check(audio: &Session, decoder: &Session) -> Result<(), SpeechRecognitionError> {
    use TensorElementType::{Float16, Int64};
    if audio.inputs().len() != 1
        || audio.outputs().len() != 1
        || decoder.inputs().len() != 7
        || decoder.outputs().len() != 3
    {
        return Err(protocol("graphs have an unsupported contract"));
    }
    for (session, name, dtype, dimensions) in [
        (audio, "input_features", Float16, &[1, 80, 3000][..]),
        (decoder, "input_ids", Int64, &[1, -1][..]),
        (decoder, "input_embeds", Float16, &[1, -1, 1024][..]),
        (decoder, "input_embed_mask", Float16, &[1, -1, 1][..]),
        (decoder, "position_ids", Int64, &[-1][..]),
        (decoder, "attention_mask", Float16, &[1, 1, -1, -1][..]),
        (decoder, "past_keys", Float16, &[28, 1, 8, -1, 128][..]),
        (decoder, "past_values", Float16, &[28, 1, 8, -1, 128][..]),
    ] {
        tensor(
            session
                .inputs()
                .iter()
                .find(|v| v.name() == name)
                .map(|v| v.dtype()),
            dtype,
            dimensions,
        )?;
    }
    for (session, name, dimensions) in [
        (audio, "audio_embeds", &[1, 375, 1024][..]),
        (decoder, "logits", &[-1, -1, 151936][..]),
        (decoder, "new_keys", &[28, -1, -1, -1, -1][..]),
        (decoder, "new_values", &[28, 1, 8, -1, 128][..]),
    ] {
        tensor(
            session
                .outputs()
                .iter()
                .find(|v| v.name() == name)
                .map(|v| v.dtype()),
            Float16,
            dimensions,
        )?;
    }
    Ok(())
}

fn tensor(
    value: Option<&ValueType>,
    dtype: TensorElementType,
    dimensions: &[i64],
) -> Result<(), SpeechRecognitionError> {
    match value {
        Some(ValueType::Tensor { ty, shape, .. })
            if *ty == dtype
                && shape.len() == dimensions.len()
                && shape
                    .iter()
                    .zip(dimensions)
                    .all(|(actual, expected)| *expected == -1 || actual == expected) =>
        {
            Ok(())
        }
        _ => Err(protocol(
            "graph tensor names, types or dimensions are unsupported",
        )),
    }
}
