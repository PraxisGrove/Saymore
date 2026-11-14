use super::{SEGMENT_SAMPLES, protocol};
use realfft::{RealFftPlanner, RealToComplex};
use std::sync::Arc;
use template_app::SpeechRecognitionError;

const FFT: usize = 400;
const BINS: usize = 80;
const FRAMES: usize = 3000;

pub(super) struct Frontend {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    filters: Vec<Vec<(usize, f32)>>,
}

impl Frontend {
    pub fn new() -> Self {
        let fft = RealFftPlanner::new().plan_fft_forward(FFT);
        let window = (0..FFT)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT as f32).cos())
            .collect();
        let maximum = hz_to_mel(8000.0);
        let edges: Vec<f64> = (0..BINS + 2)
            .map(|i| mel_to_hz(maximum * i as f64 / (BINS + 1) as f64))
            .collect();
        let filters = (0..BINS)
            .map(|mel| {
                (0..=FFT / 2)
                    .filter_map(|bin| {
                        let hz = bin as f64 * 16000.0 / FFT as f64;
                        let rising = (hz - edges[mel]) / (edges[mel + 1] - edges[mel]);
                        let falling = (edges[mel + 2] - hz) / (edges[mel + 2] - edges[mel + 1]);
                        let weight =
                            rising.min(falling).max(0.0) * 2.0 / (edges[mel + 2] - edges[mel]);
                        (weight > 0.0).then_some((bin, weight as f32))
                    })
                    .collect()
            })
            .collect();
        Self {
            fft,
            window,
            filters,
        }
    }

    pub fn features(&self, samples: &[i16]) -> Result<Vec<f32>, SpeechRecognitionError> {
        if samples.is_empty() || samples.len() > SEGMENT_SAMPLES {
            return Err(protocol(
                "frontend requires a nonempty segment of at most 30 seconds",
            ));
        }
        let mut input = self.fft.make_input_vec();
        let mut spectrum = self.fft.make_output_vec();
        let mut scratch = self.fft.make_scratch_vec();
        let mut result = vec![0.0_f32; BINS * FRAMES];
        let mut maximum = f32::NEG_INFINITY;
        for frame in 0..FRAMES {
            for (index, value) in input.iter_mut().enumerate() {
                let position = frame as isize * 160 + index as isize - 200;
                let position = if position < 0 { -position } else { position } as usize;
                let position = if position >= SEGMENT_SAMPLES {
                    2 * SEGMENT_SAMPLES - position - 2
                } else {
                    position
                };
                *value = samples
                    .get(position)
                    .map_or(0.0, |value| f32::from(*value) / 32768.0)
                    * self.window[index];
            }
            self.fft
                .process_with_scratch(&mut input, &mut spectrum, &mut scratch)
                .map_err(|_| protocol("audio FFT failed"))?;
            for (mel, filter) in self.filters.iter().enumerate() {
                let power = filter
                    .iter()
                    .map(|(bin, weight)| spectrum[*bin].norm_sqr() * weight)
                    .sum::<f32>();
                let value = power.max(1e-10).log10();
                maximum = maximum.max(value);
                result[mel * FRAMES + frame] = value;
            }
        }
        for value in &mut result {
            *value = (value.max(maximum - 8.0) + 4.0) / 4.0;
        }
        Ok(result)
    }
}

fn hz_to_mel(hz: f64) -> f64 {
    if hz < 1000.0 {
        hz * 3.0 / 200.0
    } else {
        15.0 + (hz / 1000.0).ln() / (6.4_f64.ln() / 27.0)
    }
}

fn mel_to_hz(mel: f64) -> f64 {
    if mel < 15.0 {
        mel * 200.0 / 3.0
    } else {
        1000.0 * ((mel - 15.0) * (6.4_f64.ln() / 27.0)).exp()
    }
}
