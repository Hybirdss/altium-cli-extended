//! Byte-level edits of one `|KEY=VALUE|` record.
//!
//! [`crate::props::Props`] re-serialises a whole record (and collapses duplicate keys). For
//! writes to real design files use these functions instead: they change only the bytes of the
//! field being edited, so Altium sees the rest of the record exactly as it wrote it.

use crate::props::codepage;

/// ANSI bytes for a value; characters the code page cannot represent become `?`.
pub fn encode_ansi_strict(s: &str) -> Vec<u8> {
    if s.is_ascii() {
        return s.as_bytes().to_vec();
    }
    let enc = codepage();
    let mut out = Vec::new();
    let mut buf = [0u8; 4];
    for ch in s.chars() {
        if ch.is_ascii() {
            out.push(ch as u8);
            continue;
        }
        let (b, _, errors) = enc.encode(ch.encode_utf8(&mut buf));
        if errors {
            out.push(b'?');
        } else {
            out.extend_from_slice(&b);
        }
    }
    out
}

/// Byte ranges `(field_start, value_start, value_end)` of each `|KEY=VALUE` field, where
/// `field_start` points at the key (after the `|`).
pub fn fields(data: &[u8]) -> Vec<(usize, usize, usize)> {
    let end = if data.last() == Some(&0) { data.len() - 1 } else { data.len() };
    let mut out = Vec::new();
    let mut start = 0;
    while start <= end {
        let stop = data[start..end].iter().position(|&b| b == b'|').map(|p| start + p).unwrap_or(end);
        if stop > start
            && let Some(eq) = data[start..stop].iter().position(|&b| b == b'=')
        {
            out.push((start, start + eq + 1, stop));
        }
        start = stop + 1;
    }
    out
}

pub fn key_of(data: &[u8], f: (usize, usize, usize)) -> &[u8] {
    &data[f.0..f.1 - 1]
}

pub fn find_field(data: &[u8], key: &str) -> Option<(usize, usize, usize)> {
    fields(data).into_iter().find(|&f| key_of(data, f).eq_ignore_ascii_case(key.as_bytes()))
}

/// Replace (or append) one field's value, keeping every other byte.
pub fn put_field(data: &mut Vec<u8>, key: &str, value: &[u8], before_key: Option<&str>) {
    if let Some((_, vs, ve)) = find_field(data, key) {
        data.splice(vs..ve, value.iter().copied());
        return;
    }
    let mut field = vec![b'|'];
    field.extend_from_slice(key.as_bytes());
    field.push(b'=');
    field.extend_from_slice(value);
    // Insert before another field (`%UTF8%X` goes right before `X`), else at the end.
    let at = before_key
        .and_then(|k| find_field(data, k))
        .map(|(ks, _, _)| ks - 1)
        .unwrap_or(if data.last() == Some(&0) { data.len() - 1 } else { data.len() });
    data.splice(at..at, field);
}

pub fn drop_field(data: &mut Vec<u8>, key: &str) -> bool {
    match find_field(data, key) {
        Some((ks, _, ve)) => {
            let start = if ks > 0 && data[ks - 1] == b'|' { ks - 1 } else { ks };
            data.drain(start..ve);
            true
        }
        None => false,
    }
}

/// Set a text value the way Altium stores it: ANSI under `KEY`, plus `%UTF8%KEY` when the
/// value is not ASCII or a UTF-8 copy already exists.
pub fn set_text_field(data: &mut Vec<u8>, key: &str, value: &str) {
    let utf8_key = format!("%UTF8%{key}");
    let has_utf8 = find_field(data, &utf8_key).is_some();
    if has_utf8 || !value.is_ascii() {
        if find_field(data, key).is_some() || has_utf8 {
            put_field(data, &utf8_key, value.as_bytes(), Some(key));
        } else {
            put_field(data, &utf8_key, value.as_bytes(), None);
        }
    }
    put_field(data, key, &encode_ansi_strict(value), None);
    drop_field(data, &format!("UNICODE__{key}"));
}

