/// Remove only complete timestamp/speaker wrappers; preserve malformed and
/// ordinary bracketed text so parsing cannot silently discard transcription.
pub(super) fn plain_text(raw: &str) -> String {
    let mut result = String::new();
    let mut rest = raw;
    while !rest.is_empty() {
        if let Some((_, after_start)) = timestamp(rest)
            && let Some(after_speaker) = speaker(after_start)
            && let Some((end, suffix)) = closing_timestamp(after_speaker)
        {
            result.push_str(&after_speaker[..end]);
            rest = suffix;
            continue;
        }
        let Some(ch) = rest.chars().next() else { break };
        result.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    result
}

fn timestamp(text: &str) -> Option<(&str, &str)> {
    let body = text.strip_prefix('[')?;
    let end = body.find(']')?;
    let value = &body[..end];
    if value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit() || c == b'.') {
        return None;
    }
    if !value.parse::<f64>().ok()?.is_finite() {
        return None;
    }
    Some((value, &body[end + 1..]))
}

fn speaker(text: &str) -> Option<&str> {
    let body = text.strip_prefix("[S")?;
    let end = body.find(']')?;
    if end == 0 || !body[..end].bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(&body[end + 1..])
}

fn closing_timestamp(text: &str) -> Option<(usize, &str)> {
    for (index, _) in text.match_indices('[') {
        if let Some((_, rest)) = timestamp(&text[index..]) {
            return Some((index, rest));
        }
    }
    None
}
