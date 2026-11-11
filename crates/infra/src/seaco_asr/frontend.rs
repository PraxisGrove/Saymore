use std::path::Path;

use kaldi_fbank::{FbankOptions, OnlineFbank, WindowType};
use template_app::SpeechRecognitionError;

use super::{protocol, read_text};

const MEL_BINS: usize = 80;
pub(super) const FEATURE_DIM: usize = 560;
const LFR_WINDOW: usize = 7;
const LFR_STRIDE: usize = 6;

pub(super) struct Frontend {
    shifts: Vec<f32>,
    scales: Vec<f32>,
}

impl Frontend {
    pub fn load(path: &Path) -> Result<Self, SpeechRecognitionError> {
        Self::parse(&read_text(path)?)
    }

    fn parse(text: &str) -> Result<Self, SpeechRecognitionError> {
        let shifts = cmvn_vector(text, "<AddShift>")?;
        let scales = cmvn_vector(text, "<Rescale>")?;
        if shifts.len() != FEATURE_DIM
            || scales.len() != FEATURE_DIM
            || scales.iter().any(|value| *value <= 0.0)
        {
            return Err(protocol(
                "CMVN must contain 560 finite shifts and positive scales",
            ));
        }
        Ok(Self { shifts, scales })
    }

    pub fn features(&self, samples: &[i16]) -> Result<Vec<f32>, SpeechRecognitionError> {
        if samples.is_empty() || samples.len() > super::MAX_SEGMENT_SAMPLES {
            return Err(protocol(
                "frontend requires a nonempty segment of at most 30 seconds",
            ));
        }
        let mut options = FbankOptions::default();
        options.frame_opts.dither = 0.0;
        options.frame_opts.window_type = WindowType::Hamming;
        options.mel_opts.num_bins = MEL_BINS as i32;
        // Kaldi takes PCM amplitudes, not normalized [-1, 1] waveforms.
        let mut waveform: Vec<f32> = samples.iter().map(|sample| f32::from(*sample)).collect();
        waveform.resize(waveform.len().max(400), 0.0);
        let mut fbank = OnlineFbank::new(options);
        fbank.accept_waveform(16_000.0, &waveform);
        let frames = fbank.num_frames_ready() as usize;
        if frames == 0 {
            return Err(protocol("frontend produced no frames"));
        }
        let output_frames = frames.div_ceil(LFR_STRIDE);
        let mut features = Vec::with_capacity(output_frames * FEATURE_DIM);
        for output in 0..output_frames {
            for window in 0..LFR_WINDOW {
                // Repeat the first three and final frames, exactly as FunASR LFR.
                let index = (output * LFR_STRIDE + window)
                    .saturating_sub(3)
                    .min(frames - 1);
                for (bin, value) in fbank.get_frame(index as i32).iter().enumerate() {
                    let column = window * MEL_BINS + bin;
                    features.push((value + self.shifts[column]) * self.scales[column]);
                }
            }
        }
        if features.iter().any(|value| !value.is_finite()) {
            return Err(protocol("frontend returned nonfinite features"));
        }
        Ok(features)
    }
}

fn cmvn_vector(text: &str, marker: &str) -> Result<Vec<f32>, SpeechRecognitionError> {
    let (_, tail) = text
        .split_once(marker)
        .ok_or_else(|| protocol("CMVN vector is missing"))?;
    let (_, tail) = tail
        .split_once('[')
        .ok_or_else(|| protocol("CMVN vector is malformed"))?;
    let (values, _) = tail
        .split_once(']')
        .ok_or_else(|| protocol("CMVN vector is malformed"))?;
    values
        .split_whitespace()
        .map(|token| {
            token
                .parse::<f32>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| protocol("CMVN contains an invalid number"))
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use super::*;

    #[test]
    fn short_audio_is_padded_and_invalid_cmvn_is_rejected() -> Result<(), SpeechRecognitionError> {
        let frontend = Frontend {
            shifts: vec![0.0; FEATURE_DIM],
            scales: vec![1.0; FEATURE_DIM],
        };
        let features = frontend.features(&[1])?;
        assert_eq!(FEATURE_DIM, features.len());
        assert!(features.iter().all(|value| value.is_finite()));
        assert!(Frontend::parse("<AddShift> [ NaN ] <Rescale> [ 1 ]").is_err());
        assert!(Frontend::parse("<AddShift> [ 0 ] <Rescale> [ 1 ]").is_err());
        Ok(())
    }
}
