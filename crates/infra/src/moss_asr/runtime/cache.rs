use super::super::protocol;
use half::f16;
use template_app::SpeechRecognitionError;

const HEADS: usize = 28 * 8;
const WIDTH: usize = 128;

#[derive(Default)]
pub(super) struct Cache {
    pub length: usize,
    pub keys: Vec<f16>,
    pub values: Vec<f16>,
}

impl Cache {
    pub fn append(
        &mut self,
        tokens: usize,
        keys: &[f16],
        values: &[f16],
    ) -> Result<(), SpeechRecognitionError> {
        let expected = HEADS * tokens * WIDTH;
        if keys.len() != expected || values.len() != expected || self.length + tokens > 2048 {
            return Err(protocol("decoder cache has invalid dimensions"));
        }
        self.keys = append_rows(&self.keys, self.length, keys, tokens);
        self.values = append_rows(&self.values, self.length, values, tokens);
        self.length += tokens;
        Ok(())
    }
}

fn append_rows(previous: &[f16], past: usize, new: &[f16], tokens: usize) -> Vec<f16> {
    let mut result = Vec::with_capacity(HEADS * (past + tokens) * WIDTH);
    for head in 0..HEADS {
        result.extend_from_slice(&previous[head * past * WIDTH..(head + 1) * past * WIDTH]);
        result.extend_from_slice(&new[head * tokens * WIDTH..(head + 1) * tokens * WIDTH]);
    }
    result
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use super::*;
    #[test]
    fn appends_time_rows_within_each_head() -> Result<(), SpeechRecognitionError> {
        let mut cache = Cache::default();
        let first: Vec<_> = (0..HEADS * WIDTH)
            .map(|i| f16::from_f32((i / WIDTH) as f32))
            .collect();
        let second = vec![f16::from_f32(-1.0); HEADS * WIDTH];
        cache.append(1, &first, &first)?;
        cache.append(1, &second, &second)?;
        for head in 0..HEADS {
            assert_eq!(
                &cache.keys[head * 2 * WIDTH..head * 2 * WIDTH + WIDTH],
                &first[head * WIDTH..(head + 1) * WIDTH]
            );
            assert_eq!(
                &cache.keys[head * 2 * WIDTH + WIDTH..(head + 1) * 2 * WIDTH],
                &second[..WIDTH]
            );
        }
        assert_eq!(cache.keys, cache.values);
        Ok(())
    }
}
