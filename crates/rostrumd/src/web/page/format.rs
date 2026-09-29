//! Small text helpers for the page.

use chrono::{DateTime, Utc};

/// Escape text for HTML content and double- or single-quoted attributes.
/// Device names come from phones; nothing reaches the page unescaped.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// `12.4 MB`, in SI units as a phone's download manager shows them.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["kB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = "B";
    for next in UNITS {
        if value < 1000.0 {
            break;
        }
        value /= 1000.0;
        unit = next;
    }
    format!("{value:.1} {unit}")
}

/// Hex in groups of four, so a person can compare it a chunk at a time.
pub fn group_hex(hex: &str) -> String {
    hex.as_bytes()
        .chunks(4)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The no-script fallback for a `<time>`: absolute, in UTC.
pub fn utc(at: &DateTime<Utc>) -> String {
    at.format("%Y-%m-%d %H:%M UTC").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_is_escaped() {
        assert_eq!(
            escape(r#"<img src=x onerror="alert('1')">&"#),
            "&lt;img src=x onerror=&quot;alert(&#39;1&#39;)&quot;&gt;&amp;"
        );
        assert_eq!(escape("Pixel 8"), "Pixel 8");
    }

    #[test]
    fn sizes_read_like_a_download_manager() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1_000), "1.0 kB");
        assert_eq!(human_size(12_400_000), "12.4 MB");
        assert_eq!(human_size(3_200_000_000), "3.2 GB");
    }

    #[test]
    fn hex_is_grouped_by_four() {
        assert_eq!(group_hex("0123456789abcdef01"), "0123 4567 89ab cdef 01");
    }

    #[test]
    fn times_fall_back_to_utc() {
        let at = DateTime::parse_from_rfc3339("2026-09-28T12:34:56Z")
            .expect("time")
            .with_timezone(&Utc);
        assert_eq!(utc(&at), "2026-09-28 12:34 UTC");
    }
}
