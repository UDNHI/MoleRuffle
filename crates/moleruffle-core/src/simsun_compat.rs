//! SimSun(宋体)相容的设备字体。
//!
//! 游戏绝大多数文本框指定 SimSun / 宋体(实测 61 个介面与书籍 SWF:241 个文本框里 SimSun+宋体
//! 占九成以上),排版是照 SimSun 的尺寸设计的:行高 = 1.00 em(ascent 220/256 + descent 36/256)。
//! macOS 没有 SimSun,之前回退到 PingFang TC(行高 1.40 em)或打包的 Noto Sans TC(1.45 em),
//! 固定大小的文本框里行数一多就塞不下,书本内外都会溢出。
//!
//! 对策:
//! 1. 优先用尺寸与 SimSun 一致的系统字体:SimSong(Apple 的 SimSun 相容字体,行高、中文宽、
//!    英文半宽都相同),其次 STSong(行高相同,英文较宽)。
//! 2. 都没有时,用打包的 Noto Sans TC,但在内存里把它的行高数值改成 SimSun 的比例。
//!    字形不变,只是每一行的高度与原版一致。

use std::sync::OnceLock;

/// SimSun 的垂直尺寸(em 比例):ascent 220/256、descent 36/256、无行距。
const ASCENT_RATIO: f64 = 220.0 / 256.0;
const DESCENT_RATIO: f64 = 36.0 / 256.0;

/// 依序尝试的 SimSun 相容系统字体。
pub(crate) const SYSTEM_FAMILIES: &[&str] = &["SimSong", "STSong"];

/// 游戏请求的字体名是否属于 SimSun 家族。
pub(crate) fn is_simsun(name: &str) -> bool {
    ["SimSun", "NSimSun", "宋体", "宋體", "新宋体", "新宋體"]
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
}

/// 打包字体改成 SimSun 行高后的版本(只算一次,之后共用)。改写失败则为 None。
pub(crate) fn bundled(original: &'static [u8]) -> Option<&'static [u8]> {
    static PATCHED: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    PATCHED
        .get_or_init(|| {
            let patched = with_simsun_line_metrics(original);
            if patched.is_none() {
                tracing::warn!("无法改写打包字体的行高,沿用原始尺寸");
            }
            patched
        })
        .as_deref()
}

fn table(data: &[u8], tag: &[u8; 4]) -> Option<(usize, usize)> {
    let count = u16::from_be_bytes(data.get(4..6)?.try_into().ok()?) as usize;
    (0..count).find_map(|i| {
        let record = data.get(12 + i * 16..28 + i * 16)?;
        (&record[0..4] == tag).then(|| {
            let offset = u32::from_be_bytes(record[8..12].try_into().unwrap()) as usize;
            let length = u32::from_be_bytes(record[12..16].try_into().unwrap()) as usize;
            (offset, length)
        })
    })
}

fn put_i16(data: &mut [u8], at: usize, value: i16) -> Option<()> {
    data.get_mut(at..at + 2)?.copy_from_slice(&value.to_be_bytes());
    Some(())
}

/// 把单一 TrueType/OpenType 字体的 hhea 与 OS/2 行高数值改成 SimSun 的比例。
/// 字体集合(.ttc)或缺表时回传 None。表的校验和不更新(ttf-parser 不检查)。
fn with_simsun_line_metrics(original: &[u8]) -> Option<Vec<u8>> {
    if original.get(0..4)? == b"ttcf" {
        return None;
    }
    let mut data = original.to_vec();
    let (head, _) = table(&data, b"head")?;
    let units_per_em = u16::from_be_bytes(data.get(head + 18..head + 20)?.try_into().ok()?) as f64;
    let ascent = (units_per_em * ASCENT_RATIO).round() as i16;
    let descent = (units_per_em * DESCENT_RATIO).round() as i16;

    let (hhea, hhea_len) = table(&data, b"hhea")?;
    if hhea_len < 10 {
        return None;
    }
    put_i16(&mut data, hhea + 4, ascent)?;
    put_i16(&mut data, hhea + 6, -descent)?;
    put_i16(&mut data, hhea + 8, 0)?;

    // OS/2:sTypoAscender/Descender/LineGap(68/70/72)与 usWinAscent/Descent(74/76)。
    if let Some((os2, os2_len)) = table(&data, b"OS/2") {
        if os2_len >= 78 {
            put_i16(&mut data, os2 + 68, ascent)?;
            put_i16(&mut data, os2 + 70, -descent)?;
            put_i16(&mut data, os2 + 72, 0)?;
            put_i16(&mut data, os2 + 74, ascent)?;
            put_i16(&mut data, os2 + 76, descent)?;
        }
    }
    Some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTO: &[u8] = include_bytes!("../assets/notosanstc.ttf");

    #[test]
    fn bundled_font_gets_simsun_line_height() {
        let patched = with_simsun_line_metrics(NOTO).expect("patch bundled font");
        let face = ttf_parser::Face::parse(&patched, 0).expect("patched font still parses");
        let upm = face.units_per_em() as f64;
        let line = (face.ascender() as f64 - face.descender() as f64 + face.line_gap() as f64) / upm;
        assert!((line - 1.0).abs() < 0.002, "line height {line}");
        assert_eq!(face.ascender(), (upm * ASCENT_RATIO).round() as i16);

        // 字形与字宽不变。
        let original = ttf_parser::Face::parse(NOTO, 0).unwrap();
        let id = original.glyph_index('書').unwrap();
        assert_eq!(face.glyph_index('書'), Some(id));
        assert_eq!(face.glyph_hor_advance(id), original.glyph_hor_advance(id));
    }

    #[test]
    fn collections_are_left_alone() {
        assert!(with_simsun_line_metrics(b"ttcf\0\x01\0\0").is_none());
        assert!(with_simsun_line_metrics(b"").is_none());
    }

    #[test]
    fn simsun_names() {
        assert!(is_simsun("SimSun"));
        assert!(is_simsun("simsun"));
        assert!(is_simsun("宋体"));
        assert!(!is_simsun("SimHei"));
        assert!(!is_simsun("Tahoma"));
    }
}
