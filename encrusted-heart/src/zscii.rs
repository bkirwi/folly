pub const DEFAULT_UNICODE_TABLE: &[char] = &[
    'ä', 'ö', 'ü', 'Ä', 'Ö', 'Ü', 'ß', '»', '«', 'ë', 'ï', 'ÿ', 'Ë', 'Ï', 'á', 'é', 'í', 'ó', 'ú',
    'ý', 'Á', 'É', 'Í', 'Ó', 'Ú', 'Ý', 'à', 'è', 'ì', 'ò', 'ù', 'À', 'È', 'Ì', 'Ò', 'Ù', 'â', 'ê',
    'î', 'ô', 'û', 'Â', 'Ê', 'Î', 'Ô', 'Û', 'å', 'Å', 'ø', 'Ø', 'ã', 'ñ', 'õ', 'Ã', 'Ñ', 'Õ', 'æ',
    'Æ', 'ç', 'Ç', 'þ', 'ð', 'Þ', 'Ð', '£', 'œ', 'Œ', '¡', '¿',
];

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Debug)]
pub struct ZChar(pub u8);

impl ZChar {
    pub const DELETE: ZChar = ZChar(8);
    pub const RETURN: ZChar = ZChar(13);
    pub const ESC: ZChar = ZChar(27);

    pub const UP: ZChar = ZChar(129);
    pub const DOWN: ZChar = ZChar(130);
    pub const LEFT: ZChar = ZChar(131);
    pub const RIGHT: ZChar = ZChar(132);

    pub fn keypad(digit: u8) -> ZChar {
        assert!(digit <= 9);
        ZChar(145 + digit)
    }

    pub fn from_char(ch: char, table: &[char]) -> Option<ZChar> {
        let code: u32 = ch.into();
        match code {
            0 | 8 | 9 | 27 | 32..=126 => Some(ZChar(code as u8)),
            10 | 13 => Some(ZChar::RETURN),
            11 => Some(ZChar(b' ')),
            _ => table
                .iter()
                .position(|c| *c == ch)
                .filter(|i| *i < 97)
                .map(|i| ZChar(155 + i as u8)),
        }
    }

    pub fn to_char(self, table: &[char]) -> char {
        match self.0 {
            0 | 9 | 11 | 13 | 32..=126 => self.0 as char,
            155..=251 => table.get(self.0 as usize - 155).copied().unwrap_or('?'),
            _ => '?',
        }
    }
}
