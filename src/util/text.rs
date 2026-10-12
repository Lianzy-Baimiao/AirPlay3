//! 文本编码处理。
//!
//! 原版 AirPlay 面向中文用户，大量 MP3 的 ID3 标签、LRC、CUE 是 GBK 编码。
//! 这里负责：自动识别文件编码、修复"GBK 被当成 Latin-1 解码"的乱码、生成用于匹配的归一化键。

use std::borrow::Cow;

/// 自动识别编码并解码：BOM > UTF-8 > GB18030/Big5。
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some((enc, bom_len)) = encoding_rs::Encoding::for_bom(bytes) {
        return enc.decode_without_bom_handling(&bytes[bom_len..]).0.into_owned();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let gbk = encoding_rs::GB18030.decode_without_bom_handling_and_without_replacement(bytes);
    if let Some(s) = gbk {
        return s.into_owned();
    }
    // GB18030 解码出错时再试 Big5（台湾/香港来源的歌词）
    if let Some(s) = encoding_rs::BIG5.decode_without_bom_handling_and_without_replacement(bytes) {
        return s.into_owned();
    }
    encoding_rs::GB18030.decode_without_bom_handling(bytes).0.into_owned()
}

/// 中日韩文字（含假名、谚文）。
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x20000..=0x2A6DF | 0xF900..=0xFAFF
        | 0x3040..=0x30FF | 0xAC00..=0xD7AF)
}

pub fn contains_cjk(s: &str) -> bool {
    s.chars().any(is_cjk)
}

/// 修复把 GBK（或 UTF-8）字节按 Latin-1 解码产生的乱码，例如 `ÖÜ½ÜÂ×` → `周杰伦`。
///
/// 只有全部字符都在 U+0000..=U+00FF 内的字符串才可能是这种乱码；
/// 为避免误伤 `Mötley Crüe` 这类正常西文，要求高位字节足够多或连续出现。
pub fn fix_mojibake(s: &str) -> Cow<'_, str> {
    if s.is_ascii() || s.chars().any(|c| c as u32 > 0xFF) {
        return Cow::Borrowed(s);
    }
    let bytes: Vec<u8> = s.chars().map(|c| c as u32 as u8).collect();
    if let Ok(u) = std::str::from_utf8(&bytes) {
        return Cow::Owned(u.to_string());
    }
    let high = bytes.iter().filter(|&&b| b >= 0x80).count();
    let (mut run, mut max_run) = (0, 0);
    for &b in &bytes {
        if b >= 0x80 {
            run += 1;
            max_run = max_run.max(run);
        } else {
            run = 0;
        }
    }
    if max_run < 4 && high * 2 < bytes.len() {
        return Cow::Borrowed(s);
    }
    if let Some(d) = encoding_rs::GB18030.decode_without_bom_handling_and_without_replacement(&bytes) {
        if d.chars().any(is_cjk) {
            return Cow::Owned(d.into_owned());
        }
    }
    Cow::Borrowed(s)
}

/// 全角转半角（含全角空格）。
pub fn to_halfwidth(c: char) -> char {
    match c as u32 {
        0x3000 => ' ',
        u @ 0xFF01..=0xFF5E => char::from_u32(u - 0xFEE0).unwrap_or(c),
        _ => c,
    }
}

/// 用于模糊匹配的归一化键：小写、全角转半角、只保留字母数字（含汉字）。
pub fn normalize_key(s: &str) -> String {
    s.chars()
        .map(to_halfwidth)
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// 繁体转简体（Windows 自带的 LCMapStringEx）：`蘇打綠` → `苏打绿`。没有汉字或转换失败时原样返回
pub fn to_simplified(s: &str) -> String {
    use windows::Win32::Globalization::{LCMAP_SIMPLIFIED_CHINESE, LCMapStringEx};
    if s.is_ascii() {
        return s.to_string();
    }
    let src: Vec<u16> = s.encode_utf16().collect();
    let mut out = vec![0u16; src.len() * 2 + 8];
    let n = unsafe { LCMapStringEx(windows::core::w!("zh-CN"), LCMAP_SIMPLIFIED_CHINESE, &src, Some(&mut out), None, None, windows::Win32::Foundation::LPARAM(0)) };
    if n <= 0 { s.to_string() } else { String::from_utf16_lossy(&out[..n as usize]) }
}

/// 去掉标题后面的括号说明：`暮色回响 (电影《默杀》推广曲)` → `暮色回响`。
pub fn base_title(s: &str) -> &str {
    let cut = s
        .char_indices()
        .find(|&(i, c)| i > 0 && matches!(c, '(' | '（' | '[' | '【' | '〔'))
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    s[..cut].trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_variants() {
        assert_eq!(decode_text("七月七日晴".as_bytes()), "七月七日晴");
        let (gbk, _, _) = encoding_rs::GBK.encode("周杰伦 - 晴天");
        assert_eq!(decode_text(&gbk), "周杰伦 - 晴天");
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice("歌词".as_bytes());
        assert_eq!(decode_text(&bom), "歌词");
        let utf16: Vec<u8> = [0xFF, 0xFE].into_iter()
            .chain("王菲".encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
        assert_eq!(decode_text(&utf16), "王菲");
    }

    #[test]
    fn mojibake() {
        let (gbk, _, _) = encoding_rs::GBK.encode("周杰伦");
        let latin1: String = gbk.iter().map(|&b| b as char).collect();
        assert_eq!(fix_mojibake(&latin1), "周杰伦");
        assert_eq!(fix_mojibake("Mötley Crüe"), "Mötley Crüe");
        assert_eq!(fix_mojibake("Björk"), "Björk");
        assert_eq!(fix_mojibake("Größe"), "Größe");
        assert_eq!(fix_mojibake("陈奕迅"), "陈奕迅");
        let utf8_as_latin1: String = "王菲".as_bytes().iter().map(|&b| b as char).collect();
        assert_eq!(fix_mojibake(&utf8_as_latin1), "王菲");
    }

    #[test]
    fn keys() {
        assert_eq!(normalize_key("許慧欣 - 七月七日晴！"), "許慧欣七月七日晴");
        assert_eq!(normalize_key("ＡＢＣ  Don't"), "abcdont");
        assert_eq!(base_title("暮色回响 (电影《默杀》推广曲)"), "暮色回响");
        assert_eq!(base_title("(Intro)"), "(Intro)");
    }
}
