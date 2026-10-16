//! Pure, bounded decoding of the experimentally observed native feat prefix.
//! This establishes no current-player ownership or completion evidence.
const MAX_STRING: usize = 4096;
const MAX_METADATA: u32 = 256;
const MAX_PREFIX: usize = 2 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct Definition {
    pub id: u32,
    pub category: u32,
    pub title: String,
    pub description: String,
    pub points: u32,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
    big_endian: bool,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .position
            .checked_add(count)
            .ok_or("Feat prefix overflow")?;
        if end > MAX_PREFIX {
            return Err("Feat prefix exceeds byte bound".into());
        }
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or("Truncated feat prefix")?;
        self.position = end;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn fixed32(&mut self) -> Result<u32, String> {
        let bytes: [u8; 4] = self.take(4)?.try_into().map_err(|_| "Short feat integer")?;
        Ok(if self.big_endian {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        })
    }
    fn compact(&mut self, signed: bool) -> Result<i64, String> {
        let first = self.byte()?;
        let (mut tag, mut magnitude) = if first < 16 {
            (first, 0u64)
        } else {
            (first >> 4, u64::from(first & 15))
        };
        let negative = signed && tag > 5;
        if negative {
            tag -= 5;
        }
        if tag > 5 {
            return Err("Invalid feat compact integer tag".into());
        }
        for _ in 0..tag.saturating_sub(1) {
            magnitude = (magnitude << 8) | u64::from(self.byte()?);
        }
        let limit = if signed {
            if negative {
                1u64 << 31
            } else {
                i32::MAX as u64
            }
        } else {
            u32::MAX as u64
        };
        if magnitude > limit {
            return Err("Feat compact integer overflow".into());
        }
        Ok(if negative {
            -(magnitude as i64)
        } else {
            magnitude as i64
        })
    }
    fn string(&mut self) -> Result<String, String> {
        let count = self.compact(false)? as usize;
        if count > MAX_STRING {
            return Err("Feat string exceeds size bound".into());
        }
        let text = std::str::from_utf8(self.take(count)?).map_err(|_| "Invalid feat UTF-8")?;
        if text.chars().any(char::is_control) {
            return Err("Control character in feat text".into());
        }
        Ok(text.into())
    }
    fn holder(&mut self) -> Result<String, String> {
        self.take(2)?; // Serialized holder flags; their meanings remain unproven.
        self.compact(true)?;
        let text = self.string()?;
        if self.byte()? != 0 {
            self.take(8)?;
            let count = self.compact(false)? as u32;
            if count > MAX_METADATA {
                return Err("Feat metadata exceeds entry bound".into());
            }
            for _ in 0..count {
                self.string()?;
                self.string()?;
                self.byte()?;
                self.fixed32()?;
                self.compact(true)?;
                self.compact(true)?;
                self.byte()?;
            }
        }
        Ok(text)
    }
}

/// Decode only through the point reward. Trailing objective bytes are not interpreted.
/// `big_endian` must come from validated archive flags, never from guessed values.
pub fn decode(bytes: &[u8], big_endian: bool) -> Result<Definition, String> {
    let mut cursor = Cursor {
        bytes,
        position: 0,
        big_endian,
    };
    let id = cursor.fixed32()?;
    let category = cursor.fixed32()?;
    cursor.fixed32()?; // Granted-title ID.
    cursor.fixed32()?; // Unresolved definition field.
    let title = cursor.holder()?;
    let description = cursor.holder()?;
    cursor.string()?; // Image path.
    cursor.compact(true)?; // Difficulty enum, not the point reward.
    let points = cursor.fixed32()?;
    if id == 0
        || id == u32::MAX
        || title.trim().is_empty()
        || ![0, 10, 25, 50, 100].contains(&points)
    {
        return Err("Invalid feat definition identity, title or points".into());
    }
    Ok(Definition {
        id,
        category,
        title,
        description,
        points,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixed(out: &mut Vec<u8>, value: u32, big: bool) {
        out.extend_from_slice(&if big {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        });
    }
    fn compact(out: &mut Vec<u8>, value: i64) {
        let magnitude = value.unsigned_abs();
        let payload = if magnitude <= 15 {
            0
        } else {
            ((64 - magnitude.leading_zeros() as usize - 4).div_ceil(8)).min(4)
        };
        let tag = payload + 1 + if value < 0 { 5 } else { 0 };
        out.push(((tag as u8) << 4) | ((magnitude >> (payload * 8)) as u8));
        for shift in (0..payload).rev() {
            out.push((magnitude >> (shift * 8)) as u8);
        }
    }
    fn string(out: &mut Vec<u8>, text: &[u8]) {
        compact(out, text.len() as i64);
        out.extend_from_slice(text);
    }
    fn holder(out: &mut Vec<u8>, text: &[u8], extra: bool, big: bool) {
        out.extend_from_slice(&[0, 1]);
        compact(out, -1);
        string(out, text);
        out.push(u8::from(extra));
        if extra {
            out.extend_from_slice(&[0; 8]);
            compact(out, 1);
            string(out, b"key");
            string(out, b"value");
            out.push(1);
            fixed(out, 0x12345678, big);
            compact(out, i32::MIN as i64);
            compact(out, i32::MAX as i64);
            out.push(0);
        }
    }
    fn fixture(big: bool, extra: bool) -> Vec<u8> {
        let mut out = Vec::new();
        for value in [1234567, 12, u32::MAX, 0] {
            fixed(&mut out, value, big);
        }
        holder(&mut out, "Synthetic café".as_bytes(), extra, big);
        holder(&mut out, b"Do a synthetic thing", extra, big);
        string(&mut out, b"img/test");
        compact(&mut out, 30);
        fixed(&mut out, 25, big);
        out
    }
    #[test]
    fn decodes_both_endianness_and_optional_metadata() {
        for big in [false, true] {
            for extra in [false, true] {
                let bytes = fixture(big, extra);
                let value = decode(&bytes, big).unwrap();
                assert_eq!((value.id, value.category, value.points), (1234567, 12, 25));
                assert_eq!(value.title, "Synthetic café");
                assert_eq!(value.description, "Do a synthetic thing");
                let mut trailing = bytes;
                trailing.extend_from_slice(&[0xff; 10]);
                assert_eq!(decode(&trailing, big).unwrap(), value);
            }
        }
    }
    #[test]
    fn every_truncation_fails() {
        let bytes = fixture(true, true);
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end], true).is_err(), "{end}");
        }
    }
    #[test]
    fn compact_boundaries_and_invalid_encodings() {
        for value in [
            0,
            1,
            15,
            16,
            255,
            4095,
            4096,
            i32::MAX as i64,
            i32::MIN as i64,
        ] {
            let mut bytes = Vec::new();
            compact(&mut bytes, value);
            let mut cursor = Cursor {
                bytes: &bytes,
                position: 0,
                big_endian: false,
            };
            assert_eq!(cursor.compact(true).unwrap(), value);
            assert_eq!(cursor.position, bytes.len());
        }
        for bytes in [
            &[0xf0][..],
            &[0x5f, 255, 255, 255, 255],
            &[0x58, 0, 0, 0, 0],
        ] {
            assert!(
                Cursor {
                    bytes,
                    position: 0,
                    big_endian: false
                }
                .compact(true)
                .is_err()
            );
        }
        let mut bytes = Vec::new();
        compact(&mut bytes, u32::MAX as i64);
        assert_eq!(
            Cursor {
                bytes: &bytes,
                position: 0,
                big_endian: false
            }
            .compact(false)
            .unwrap(),
            u32::MAX as i64
        );
    }
    #[test]
    fn rejects_bad_text_points_and_bounds() {
        for text in [&[255][..], b"bad\ntext", b"bad\0text"] {
            let mut bytes = Vec::new();
            string(&mut bytes, text);
            assert!(
                Cursor {
                    bytes: &bytes,
                    position: 0,
                    big_endian: false
                }
                .string()
                .is_err()
            );
        }
        let mut bytes = fixture(false, false);
        let end = bytes.len();
        bytes[end - 4..].copy_from_slice(&11u32.to_le_bytes());
        assert!(decode(&bytes, false).is_err());
        let mut bytes = Vec::new();
        compact(&mut bytes, 4097);
        assert!(
            Cursor {
                bytes: &bytes,
                position: 0,
                big_endian: false
            }
            .string()
            .is_err()
        );
        let mut bytes = vec![0, 0, 0, 0, 1];
        bytes.extend_from_slice(&[0; 8]);
        compact(&mut bytes, 257);
        assert!(
            Cursor {
                bytes: &bytes,
                position: 0,
                big_endian: false
            }
            .holder()
            .is_err()
        );
    }
}
