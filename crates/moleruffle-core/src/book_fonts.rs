//! Book-only device-font aliases. UI resources keep the original fallback policy.
use std::io::Read;

const PREFIX: &str = "MoleRuffle Book: ";
const SERIF: &[&str] = &[
    "STSong",
    "Songti TC",
    "Songti SC",
    "Noto Serif CJK TC",
    "Noto Serif CJK SC",
];
const SANS: &[&str] = &[
    "Heiti TC",
    "Heiti SC",
    "PingFang TC",
    "Microsoft JhengHei",
    "Noto Sans CJK TC",
];

fn base_family(name: &str) -> &str {
    if let Some((family, size)) = name.strip_suffix("_st").and_then(|s| s.rsplit_once('_')) {
        if size
            .strip_suffix("pt")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
        {
            return family;
        }
    }
    name
}

fn candidates(name: &str) -> Option<&'static [&'static str]> {
    let name = base_family(name);
    if [
        "SimSun",
        "NSimSun",
        "宋体",
        "宋體",
        "PMingLiU",
        "MingLiU",
        "新細明體",
        "細明體",
        "Times New Roman",
        "Times",
        "_serif",
    ]
    .iter()
    .any(|n| name.eq_ignore_ascii_case(n))
    {
        return Some(SERIF);
    }
    if [
        "Arial",
        "Helvetica",
        "Tahoma",
        "Verdana",
        "SimHei",
        "黑体",
        "黑體",
        "_sans",
    ]
    .iter()
    .any(|n| name.eq_ignore_ascii_case(n))
    {
        return Some(SANS);
    }
    None
}

pub(crate) fn families(name: &str) -> Option<&'static [&'static str]> {
    candidates(name.strip_prefix(PREFIX)?)
}

fn alias(name: &str) -> Option<String> {
    candidates(name)?;
    Some(format!("{PREFIX}{name}"))
}

pub(crate) fn applies(url: &url::Url) -> bool {
    if url.host_str() != Some("mole.61.com.tw") {
        return false;
    }
    let path = url.path();
    if matches!(
        path,
        "/module/external/BooksUI/ConstructionBook.swf"
            | "/module/external/XihaBook.swf"
            | "/module/external/heroWarBook.swf"
            | "/module/external/tenYearsAgo.swf"
    ) {
        return true;
    }
    let Some(file) = path
        .strip_prefix("/resource/besmearBook/other/")
        .or_else(|| path.strip_prefix("/resource/besmearBook/"))
    else {
        return false;
    };
    let Some(id) = file.strip_suffix(".swf") else {
        return false;
    };
    // 27 is 伊蓮小理財, explicitly excluded by the user.
    !id.is_empty()
        && id.bytes().all(|c| c.is_ascii_digit())
        && id.parse::<u32>().is_ok_and(|n| n != 27)
}

struct Tag<'a> {
    code: u16,
    bytes: &'a [u8],
    raw: &'a [u8],
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn tags(bytes: &[u8]) -> Option<Vec<Tag<'_>>> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let start = offset;
        let header = u16_at(bytes, offset)?;
        offset += 2;
        let mut len = (header & 63) as usize;
        if len == 63 {
            len = u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?) as usize;
            offset += 4;
        }
        let body = bytes.get(offset..offset.checked_add(len)?)?;
        offset += len;
        out.push(Tag {
            code: header >> 6,
            bytes: body,
            raw: &bytes[start..offset],
        });
        if header >> 6 == 0 {
            // Do not drop unusual trailing bytes.
            if offset != bytes.len() {
                return None;
            }
            break;
        }
    }
    Some(out)
}

fn rect_len(bytes: &[u8]) -> Option<usize> {
    let count = (*bytes.first()? >> 3) as usize;
    let len = (5 + 4 * count).div_ceil(8);
    bytes.get(..len)?;
    Some(len)
}

fn emit(out: &mut Vec<u8>, code: u16, bytes: &[u8]) -> Option<()> {
    let len = u32::try_from(bytes.len()).ok()?;
    out.extend_from_slice(&((code << 6) | 63).to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(bytes);
    Some(())
}

fn string_end(bytes: &[u8], offset: usize) -> Option<usize> {
    Some(offset + bytes.get(offset..)?.iter().position(|b| *b == 0)?)
}

// Only face attribute values change; paragraph text, spaces and HTML formatting
// remain verbatim. Both quotation styles and attribute casing are supported.
fn html_faces(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::new();
    let (mut pos, mut copied) = (0, 0);
    let mut in_tag = false;
    while pos < b.len() {
        if b[pos] == b'<' {
            in_tag = true;
        }
        if b[pos] == b'>' {
            in_tag = false;
        }
        if in_tag
            && b.get(pos..pos + 4)
                .is_some_and(|s| s.eq_ignore_ascii_case(b"face"))
            && pos > 0
            && b[pos - 1].is_ascii_whitespace()
        {
            let mut p = pos + 4;
            while b.get(p).is_some_and(u8::is_ascii_whitespace) {
                p += 1;
            }
            if b.get(p) == Some(&b'=') {
                p += 1;
                while b.get(p).is_some_and(u8::is_ascii_whitespace) {
                    p += 1;
                }
                if let Some(quote @ (b'\'' | b'"')) = b.get(p).copied() {
                    let start = p + 1;
                    if let Some(end) = b
                        .get(start..)
                        .and_then(|s| s.iter().position(|c| *c == quote))
                        .map(|n| start + n)
                    {
                        if let Some(name) = text.get(start..end).and_then(alias) {
                            out.push_str(&text[copied..start]);
                            out.push_str(&name);
                            copied = end;
                        }
                        pos = end;
                    }
                }
            }
        }
        pos += 1;
    }
    out.push_str(&text[copied..]);
    out
}

fn text_position(field: &[u8]) -> Option<(usize, u16)> {
    let flags_offset = 2 + rect_len(field.get(2..)?)?;
    let flags = u16_at(field, flags_offset)?;
    let mut p = flags_offset + 2;
    if flags & 1 != 0 {
        p += 2;
    }
    if flags & 0x8000 != 0 {
        p = string_end(field, p)? + 1;
    }
    if flags & 0x8001 != 0 {
        p += 2;
    }
    if flags & 4 != 0 {
        p += 4;
    }
    if flags & 2 != 0 {
        p += 2;
    }
    if flags & 0x2000 != 0 {
        p += 9;
    }
    p = string_end(field, p)? + 1;
    Some((p, flags))
}

fn rewrite(bytes: &[u8], out: &mut Vec<u8>, changed: &mut bool, depth: usize) -> Option<()> {
    if depth > 64 {
        return None;
    }
    for tag in tags(bytes)? {
        match tag.code {
            39 => {
                let mut body = tag.bytes.get(..4)?.to_vec();
                let mut nested_changed = false;
                rewrite(
                    tag.bytes.get(4..)?,
                    &mut body,
                    &mut nested_changed,
                    depth + 1,
                )?;
                if nested_changed {
                    emit(out, tag.code, &body)?;
                    *changed = true;
                } else {
                    out.extend_from_slice(tag.raw);
                }
            }
            48 | 75 => {
                let end = 5 + *tag.bytes.get(4)? as usize;
                let name = std::str::from_utf8(tag.bytes.get(5..end)?)
                    .ok()?
                    .trim_end_matches('\0');
                if let Some(name) = alias(name) {
                    let mut body = tag.bytes.get(..4)?.to_vec();
                    body.push(u8::try_from(name.len()).ok()?);
                    body.extend_from_slice(name.as_bytes());
                    body.extend_from_slice(tag.bytes.get(end..)?);
                    emit(out, tag.code, &body)?;
                    *changed = true;
                } else {
                    out.extend_from_slice(tag.raw);
                }
            }
            37 => {
                let (start, flags) = text_position(tag.bytes)?;
                if flags & 0x280 == 0x280 {
                    // HTML faces must follow renamed definitions even when
                    // this field uses embedded outlines or mixes font types.
                    let end = string_end(tag.bytes, start)?;
                    let text = std::str::from_utf8(tag.bytes.get(start..end)?).ok()?;
                    let html = html_faces(text);
                    if html != text {
                        let mut body = tag.bytes.get(..start)?.to_vec();
                        body.extend_from_slice(html.as_bytes());
                        body.extend_from_slice(tag.bytes.get(end..)?);
                        emit(out, tag.code, &body)?;
                        *changed = true;
                    } else {
                        out.extend_from_slice(tag.raw);
                    }
                } else {
                    out.extend_from_slice(tag.raw);
                }
            }
            _ => out.extend_from_slice(tag.raw),
        }
    }
    Some(())
}

/// Work on response bytes only; the caller retains the original cache files.
/// Invalid/unsupported resources are served unchanged.
pub(crate) fn patch(bytes: &[u8]) -> Option<Vec<u8>> {
    let length = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
    if !(8..=32 * 1024 * 1024).contains(&length) {
        return None;
    }
    let data = match bytes.get(..3)? {
        b"FWS" => bytes.get(8..)?.to_vec(),
        b"CWS" => {
            let mut data = Vec::new();
            flate2::read::ZlibDecoder::new(bytes.get(8..)?)
                .take(length as u64)
                .read_to_end(&mut data)
                .ok()?;
            data
        }
        _ => return None,
    };
    if data.len() != length - 8 {
        return None;
    }
    let start = rect_len(&data)? + 4;
    let mut out = b"FWS".to_vec();
    out.extend_from_slice(bytes.get(3..8)?);
    out.extend_from_slice(data.get(..start)?);
    let mut changed = false;
    rewrite(data.get(start..)?, &mut out, &mut changed, 0)?;
    if !changed {
        return None;
    }
    let len = u32::try_from(out.len()).ok()?;
    out[4..8].copy_from_slice(&len.to_le_bytes());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_excludes_ui_and_finance_book() {
        for path in [
            "/resource/ui/index.swf",
            "/resource/ui/main_resource.swf",
            "/resource/besmearBook/other/27.swf",
            "/resource/besmearBook/027.swf",
            "/resource/besmearBook/other/besmearBook.swf",
        ] {
            assert!(!applies(
                &format!("http://mole.61.com.tw{path}").parse().unwrap()
            ));
        }
        assert!(applies(
            &"http://mole.61.com.tw/resource/besmearBook/other/28.swf?v=1"
                .parse()
                .unwrap()
        ));
        assert!(!applies(
            &"http://example.com/resource/besmearBook/other/28.swf"
                .parse()
                .unwrap()
        ));
    }
    #[test]
    fn ordinary_queries_never_use_book_metrics() {
        for name in ["SimSun", "宋体", "Arial", "Tahoma", "_sans", "宋体_14pt_st"] {
            assert!(families(name).is_none());
            assert!(families(&alias(name).unwrap()).is_some());
        }
    }
    #[test]
    fn html_rewrites_font_attributes_only() {
        let html = "<FONT FACE = '宋体_14pt_st'>文字 face=\"Arial\"</FONT><font face=\"Tahoma\">內文</font>";
        let out = html_faces(html);
        assert_eq!(
            out,
            "<FONT FACE = 'MoleRuffle Book: 宋体_14pt_st'>文字 face=\"Arial\"</FONT><font face=\"MoleRuffle Book: Tahoma\">內文</font>"
        );
        assert_eq!(html_faces(&out), out);
    }
    #[test]
    fn truncated_input_fails_safely() {
        for b in [b"".as_slice(), b"CWS", b"FWS\x09\x10\0\0\0"] {
            assert!(patch(b).is_none());
        }
    }
}
