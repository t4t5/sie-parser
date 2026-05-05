//! CP437 (IBM PC-8) encoding/decoding. The SIE 4B spec §5.8 mandates this
//! encoding for files declaring `#FORMAT PC8`.
//!
//! The low half (0x00–0x7F) is identical to ASCII. The high half mapping is
//! transcribed from <https://en.wikipedia.org/wiki/Code_page_437> — specifically
//! the "Unicode equivalents" column.

use crate::Encoding;
use anyhow::{Result, bail};

const CP437_HIGH: [char; 128] = [
    // 0x80..=0x8F
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å',
    // 0x90..=0x9F
    'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ',
    // 0xA0..=0xAF
    'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»',
    // 0xB0..=0xBF
    '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐',
    // 0xC0..=0xCF
    '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧',
    // 0xD0..=0xDF
    '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀',
    // 0xE0..=0xEF
    'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩',
    // 0xF0..=0xFF
    '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{00A0}',
];

pub fn decode_cp437(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b < 0x80 {
            out.push(b as char);
        } else {
            out.push(CP437_HIGH[(b - 0x80) as usize]);
        }
    }
    out
}

/// Encode a UTF-8 string as CP437 bytes. Errors on characters not in the
/// code page rather than silently dropping — a silent drop in a SIE writer
/// would corrupt org numbers or company names.
pub fn encode_cp437(s: &str) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        match cp437_byte(c) {
            Some(b) => out.push(b),
            None => bail!(
                "character {c:?} (U+{:04X}) is not representable in CP437",
                c as u32
            ),
        }
    }
    Ok(out)
}

fn cp437_byte(c: char) -> Option<u8> {
    if (c as u32) < 0x80 {
        return Some(c as u8);
    }
    CP437_HIGH
        .iter()
        .position(|&h| h == c)
        .map(|i| 0x80 + i as u8)
}

/// Sniff encoding. If the file contains `#FORMAT PC8` near the top, treat as
/// CP437. Otherwise, if the bytes are valid UTF-8, treat as UTF-8. Fall back
/// to CP437.
pub fn detect_encoding(bytes: &[u8]) -> Encoding {
    let head_len = bytes.len().min(4096);
    let head = &bytes[..head_len];
    if head
        .windows(11)
        .any(|w| w.eq_ignore_ascii_case(b"#FORMAT PC8"))
    {
        return Encoding::Cp437;
    }
    match std::str::from_utf8(bytes) {
        Ok(_) => Encoding::Utf8,
        Err(_) => Encoding::Cp437,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_128_entries() {
        assert_eq!(CP437_HIGH.len(), 128);
    }

    #[test]
    fn ascii_passthrough() {
        assert_eq!(decode_cp437(b"hello 123"), "hello 123");
        assert_eq!(encode_cp437("hello 123").unwrap(), b"hello 123");
    }

    #[test]
    fn swedish_letters() {
        // CP437: Å=0x8F, Ä=0x8E, Ö=0x99, å=0x86, ä=0x84, ö=0x94
        assert_eq!(decode_cp437(&[0x8F, 0x8E, 0x99]), "ÅÄÖ");
        assert_eq!(decode_cp437(&[0x86, 0x84, 0x94]), "åäö");
        assert_eq!(encode_cp437("ÅÄÖ").unwrap(), vec![0x8F, 0x8E, 0x99]);
        assert_eq!(encode_cp437("åäö").unwrap(), vec![0x86, 0x84, 0x94]);
    }

    #[test]
    fn roundtrip() {
        let s = "Årets resultat";
        let bytes = encode_cp437(s).unwrap();
        assert_eq!(decode_cp437(&bytes), s);
    }

    #[test]
    fn rejects_non_cp437() {
        assert!(encode_cp437("😀").is_err());
        assert!(encode_cp437("→").is_err());
    }

    #[test]
    fn detects_cp437_via_format_marker() {
        let s = b"#FLAGGA 0\r\n#FORMAT PC8\r\n#SIETYP 4\r\n";
        assert!(matches!(detect_encoding(s), Encoding::Cp437));
    }

    #[test]
    fn detects_utf8_when_valid() {
        assert!(matches!(detect_encoding(b"plain ascii"), Encoding::Utf8));
    }

    #[test]
    fn falls_back_to_cp437_on_invalid_utf8() {
        assert!(matches!(
            detect_encoding(&[0xFF, 0xFE, 0xFD]),
            Encoding::Cp437
        ));
    }
}
