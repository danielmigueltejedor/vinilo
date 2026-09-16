// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lyrics for the expanded player. Each catalogue has its own endpoint; this
//! file is the door the daemon uses so the GTK client never talks HTTP.

use anyhow::{Result, bail};
use serde_json::Value;

use crate::i18n::{self, Key};
use crate::ipc::{LyricLine, Lyrics};
use crate::music::client::Client;
use crate::streams::StreamHit;

pub async fn for_id(http: &reqwest::Client, apple: Option<&Client>, id: &str) -> Result<Lyrics> {
    if id.starts_with("sp:") {
        crate::spotify::lyrics(http, id).await
    } else if id.starts_with("yt:") {
        crate::ytmusic::lyrics(http, id).await
    } else if StreamHit::is_stream_id(id) {
        bail!("{}", i18n::t(Key::LyricsMissing))
    } else {
        let Some(apple) = apple else {
            bail!("{}", i18n::t(Key::LyricsMissing));
        };
        apple.lyrics(id).await
    }
}

/// Apple's lyrics resources carry a TTML blob, not JSON lines.
pub(crate) fn from_apple_ttml(value: &Value) -> Option<Lyrics> {
    lines_from_ttml(ttml_blob(value)?)
}

fn ttml_blob(value: &Value) -> Option<&str> {
    if let Some(s) = ttml_in_attrs(value.get("attributes")) {
        return Some(s);
    }
    if let Some(rel) = value.get("relationships") {
        for key in ["syllable-lyrics", "lyrics"] {
            if let Some(s) = rel.get(key).and_then(ttml_blob) {
                return Some(s);
            }
        }
    }
    let items = match value {
        Value::Array(items) => items.as_slice(),
        _ => value.get("data")?.as_array()?.as_slice(),
    };
    items.iter().find_map(ttml_blob)
}

fn ttml_in_attrs(attrs: Option<&Value>) -> Option<&str> {
    let attrs = attrs?;
    for key in ["ttml", "ttmlLocalizations"] {
        match attrs.get(key) {
            Some(Value::String(s)) if looks_like_ttml(s) => return Some(s),
            Some(Value::Object(map)) => {
                if let Some(s) = map
                    .values()
                    .find_map(|v| v.as_str().filter(|text| looks_like_ttml(text)))
                {
                    return Some(s);
                }
            }
            _ => {}
        }
    }
    None
}

fn looks_like_ttml(s: &str) -> bool {
    s.contains("<p") || s.contains("<tt")
}

fn lines_from_ttml(ttml: &str) -> Option<Lyrics> {
    let mut lines = Vec::new();
    let mut rest = ttml;
    while let Some(start) = rest.find("<p") {
        rest = &rest[start + 2..];
        let Some(tag_end) = rest.find('>') else {
            break;
        };
        let attrs = &rest[..tag_end];
        rest = &rest[tag_end + 1..];
        let Some(close) = rest.find("</p>") else {
            break;
        };
        let inner = &rest[..close];
        rest = &rest[close + 4..];
        let text = plain_text(inner);
        if text.is_empty() {
            continue;
        }
        let start_ms = attr(attrs, "begin")
            .or_else(|| attr(attrs, "in"))
            .and_then(ttml_ms)
            .unwrap_or(0);
        lines.push(LyricLine { start_ms, text });
    }
    if lines.is_empty() {
        return None;
    }
    Some(Lyrics {
        synced: lines.iter().any(|l| l.start_ms > 0),
        lines,
    })
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=");
    let rest = tag.get(tag.find(&needle)? + needle.len()..)?;
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = rest.get(1..)?;
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

fn ttml_ms(raw: &str) -> Option<u64> {
    let s = raw.trim().trim_end_matches('s');
    let parts: Vec<&str> = s.split(':').collect();
    let num = |p: &str| p.parse::<f64>().ok();
    let seconds = match parts.as_slice() {
        [h, m, sec] => num(h)? * 3600.0 + num(m)? * 60.0 + num(sec)?,
        [m, sec] => num(m)? * 60.0 + num(sec)?,
        [sec] => num(sec)?,
        _ => return None,
    };
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some((seconds * 1000.0).round() as u64)
}

fn plain_text(inner: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in inner.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let unescaped = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'");
    unescaped.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttml_clock_time_and_syllable_spans() {
        let json = serde_json::json!({
            "data": [{
                "attributes": {
                    "ttml": r#"<tt><body><div>
                        <p begin="00:00:01.200" end="00:00:03.000">
                            <span begin="00:00:01.200">Hola</span>
                            <span begin="00:00:01.800">mundo</span>
                        </p>
                        <p begin="00:00:04.000">Noche</p>
                    </div></body></tt>"#
                }
            }]
        });
        let lyrics = from_apple_ttml(&json).unwrap();
        assert!(lyrics.synced);
        assert_eq!(lyrics.lines.len(), 2);
        assert_eq!(lyrics.lines[0].text, "Hola mundo");
        assert_eq!(lyrics.lines[0].start_ms, 1200);
        assert_eq!(lyrics.lines[1].start_ms, 4000);
    }

    #[test]
    fn unsynced_ttml_has_no_times() {
        let json = serde_json::json!({
            "data": [{
                "attributes": {
                    "ttml": "<tt><body><div><p>One</p><p>Two</p></div></body></tt>"
                }
            }]
        });
        let lyrics = from_apple_ttml(&json).unwrap();
        assert!(!lyrics.synced);
        assert_eq!(lyrics.lines[0].start_ms, 0);
        assert_eq!(lyrics.lines[1].text, "Two");
    }

    #[test]
    fn empty_ttml_is_missing() {
        let json = serde_json::json!({"data":[{"attributes":{"ttml":"<tt></tt>"}}]});
        assert!(from_apple_ttml(&json).is_none());
    }

    #[test]
    fn seconds_suffix_parses() {
        assert_eq!(ttml_ms("12.5s"), Some(12_500));
        assert_eq!(ttml_ms("1:02.000"), Some(62_000));
    }

    #[test]
    fn include_lyrics_nests_the_ttml() {
        let json = serde_json::json!({
            "data": [{
                "type": "songs",
                "relationships": {
                    "lyrics": {
                        "data": [{
                            "attributes": {
                                "ttml": "<tt><body><div><p begin=\"1.5s\">Hola</p></div></body></tt>"
                            }
                        }]
                    }
                }
            }]
        });
        let lyrics = from_apple_ttml(&json).unwrap();
        assert_eq!(lyrics.lines[0].text, "Hola");
        assert_eq!(lyrics.lines[0].start_ms, 1500);
    }

    #[test]
    fn ttml_localizations_string_is_used() {
        let json = serde_json::json!({
            "data": [{
                "attributes": {
                    "ttmlLocalizations": "<tt><body><div><p begin=\"0.064\">Uno</p></div></body></tt>"
                }
            }]
        });
        let lyrics = from_apple_ttml(&json).unwrap();
        assert_eq!(lyrics.lines[0].text, "Uno");
        assert_eq!(lyrics.lines[0].start_ms, 64);
    }
}
