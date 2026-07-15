use anyhow::{Context, Result, bail};

pub fn pcm16_mono_16khz(bytes: &[u8]) -> Result<Vec<i16>> {
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        bail!("recording is not a RIFF WAVE file");
    }
    let mut offset = 12_usize;
    let mut valid_format = false;
    let mut samples = None;
    while offset.saturating_add(8) <= bytes.len() {
        let identifier = bytes
            .get(offset..offset + 4)
            .context("WAV chunk id is truncated")?;
        let size = bytes
            .get(offset + 4..offset + 8)
            .and_then(|value| <[u8; 4]>::try_from(value).ok())
            .map(u32::from_le_bytes)
            .and_then(|value| usize::try_from(value).ok())
            .context("WAV chunk size is invalid")?;
        let start = offset + 8;
        let end = start.checked_add(size).context("WAV chunk overflows")?;
        let chunk = bytes.get(start..end).context("WAV chunk is truncated")?;
        match identifier {
            b"fmt " => valid_format = valid_pcm_format(chunk),
            b"data" => samples = Some(decode_samples(chunk)?),
            _ => {}
        }
        offset = end.saturating_add(size % 2);
    }
    if !valid_format {
        bail!("recording must be 16 kHz mono PCM16");
    }
    samples.context("WAV data chunk is missing")
}

fn valid_pcm_format(chunk: &[u8]) -> bool {
    let read_u16 = |offset| {
        chunk
            .get(offset..offset + 2)
            .and_then(|value| <[u8; 2]>::try_from(value).ok())
            .map(u16::from_le_bytes)
    };
    let sample_rate = chunk
        .get(4..8)
        .and_then(|value| <[u8; 4]>::try_from(value).ok())
        .map(u32::from_le_bytes);
    read_u16(0) == Some(1)
        && read_u16(2) == Some(1)
        && sample_rate == Some(16_000)
        && read_u16(14) == Some(16)
}

fn decode_samples(chunk: &[u8]) -> Result<Vec<i16>> {
    if !chunk.len().is_multiple_of(2) {
        bail!("PCM16 data has an odd byte count");
    }
    chunk
        .chunks_exact(2)
        .map(|value| {
            <[u8; 2]>::try_from(value)
                .map(i16::from_le_bytes)
                .context("PCM16 sample is invalid")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_recorder_wav_contract() -> Result<()> {
        let mut bytes = Vec::from(b"RIFF\x28\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00\x80\x3e\x00\x00\x00\x7d\x00\x00\x02\x00\x10\x00data\x04\x00\x00\x00".as_slice());
        bytes.extend_from_slice(&[1, 0, 254, 255]);

        assert_eq!(vec![1, -2], pcm16_mono_16khz(&bytes)?);
        Ok(())
    }

    #[test]
    fn rejects_an_unsupported_sample_rate() {
        let bytes = b"RIFF\x24\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00\x44\xac\x00\x00\x88\x58\x01\x00\x02\x00\x10\x00data\x00\x00\x00\x00";
        assert!(pcm16_mono_16khz(bytes).is_err());
    }
}
