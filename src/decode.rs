//! Bytes -> text for trees written by anything: UTF-8 (± BOM), UTF-16 (± BOM),
//! or code page 437 (`tree /F > x.txt` from a Windows cmd, copied over).

const CP437_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»\
░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀\
αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, u16::from_be_bytes);
    }

    // BOM-less UTF-16: mostly-ASCII text has a zero in every other byte
    // (not "no zeros at all" on the other side: U+2500 ─ has a zero low byte)
    if bytes.len() >= 4 {
        let odd = bytes.iter().skip(1).step_by(2).filter(|&&b| b == 0).count();
        let even = bytes.iter().step_by(2).filter(|&&b| b == 0).count();
        if odd > bytes.len() / 4 && even * 4 < odd {
            return utf16(bytes, u16::from_le_bytes);
        }
        if even > bytes.len() / 4 && odd * 4 < even {
            return utf16(bytes, u16::from_be_bytes);
        }
    }

    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => {
            let high: Vec<char> = CP437_HIGH.chars().collect();
            bytes.iter().map(|&b| if b < 0x80 { b as char } else { high[b as usize - 0x80] }).collect()
        }
    }
}

fn utf16(bytes: &[u8], f: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|&c| f(c)).collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "demo/\n├── a.txt\n└── b/\n";

    fn utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn table_is_complete() {
        assert_eq!(CP437_HIGH.chars().count(), 128);
    }

    #[test]
    fn utf8_with_and_without_bom() {
        assert_eq!(decode(SAMPLE.as_bytes()), SAMPLE);
        assert_eq!(decode(&[&[0xEF, 0xBB, 0xBF][..], SAMPLE.as_bytes()].concat()), SAMPLE);
    }

    #[test]
    fn utf16_with_and_without_bom() {
        assert_eq!(decode(&[&[0xFF, 0xFE][..], &utf16le(SAMPLE)].concat()), SAMPLE);
        assert_eq!(decode(&utf16le(SAMPLE)), SAMPLE);
        let be: Vec<u8> = SAMPLE.encode_utf16().flat_map(u16::to_be_bytes).collect();
        assert_eq!(decode(&be), SAMPLE);
    }

    #[test]
    fn cp437() {
        // "├── a" in code page 437
        assert_eq!(decode(&[0xC3, 0xC4, 0xC4, b' ', b'a']), "├── a");
    }
}
