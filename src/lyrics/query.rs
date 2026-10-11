//! 在线搜索歌词前清理标题 / 艺人。
//!
//! 本地曲库的标签经常带着版本说明和来源水印，例如 `体面 (Live)`、`漂洋过海来看你【六倍音质】`、
//! `亲爱的你的棉裤真的不在我家[music.migu.cn]`、`陈楚生/王赫野`。原样发给 LRCLIB 基本搜不到，
//! 所以只在拼查询参数时清理；保存文件、本地匹配仍使用原始标签。

/// 成对的括号：其中的内容整段去掉
const OPEN: &[char] = &['(', '（', '[', '【', '〔', '〖', '{', '｛', '「', '『'];
const CLOSE: &[char] = &[')', '）', ']', '】', '〕', '〗', '}', '｝', '」', '』'];

/// ` - ` 之后出现这些词时视为版本说明（`我的热爱 - 伴奏`、`xxx - Live`）
const VERSION_WORDS: &[&str] = &[
    "伴奏", "live", "现场", "remix", "dj", "翻唱", "cover", "纯音乐", "instrumental", "karaoke", "伴唱", "吉他版", "钢琴版", "加速版", "降调", "升调",
    "女声版", "男声版", "完整版", "片段", "试听", "铃声", "remaster",
];

/// 去掉括号及其中内容；全被括住（如 `(Intro)`）时保留原文
fn strip_brackets(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        if OPEN.contains(&c) {
            depth += 1;
        } else if CLOSE.contains(&c) {
            depth = depth.saturating_sub(1);
        } else if depth == 0 {
            out.push(c);
        }
    }
    let t = squash(&out);
    if t.is_empty() { squash(s) } else { t }
}

/// 合并连续空白、去掉首尾空白和多余的分隔符
fn squash(s: &str) -> String {
    let joined = s.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.trim_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | '·' | '•' | '|' | ',' | '，')).to_string()
}

/// 去掉开头的曲目编号：`1081.爱情转移`、`01 - 晴天`、`3、七里香`
fn strip_track_no(s: &str) -> &str {
    let digits = s.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 4 {
        return s;
    }
    let rest = s[digits..].trim_start_matches(' ');
    let mut it = rest.chars();
    match it.next() {
        Some('.' | '．' | '、' | '-' | '_') => {
            let r = it.as_str().trim_start();
            if r.is_empty() { s } else { r }
        }
        _ => s,
    }
}

/// 标题：去括号说明、曲目编号、`.mp3` 之类残留、结尾的 ` - 伴奏` 版本说明
pub fn clean_title(raw: &str) -> String {
    let s = strip_brackets(raw);
    let s = strip_track_no(&s).replace('_', " ");
    let mut s = squash(&s);
    // ` - ` 后面是版本说明时截掉（`Sekiro - Enemy alerted sound effect` 这种正常标题不动）
    if let Some(i) = s.rfind(" - ") {
        let tail = s[i + 3..].to_lowercase();
        if VERSION_WORDS.iter().any(|w| tail.contains(w)) && !s[..i].trim().is_empty() {
            s.truncate(i);
            s = squash(&s);
        }
    }
    if s.is_empty() { squash(raw) } else { s }
}

/// 艺人：去掉水印，多位艺人只取第一位（LRCLIB 的艺人名通常只有主唱）
pub fn clean_artist(raw: &str) -> String {
    let s = strip_brackets(raw);
    let lower = s.to_lowercase();
    let mut cut = s.len();
    for sep in [" feat.", " feat ", " ft.", " featuring "] {
        if let Some(i) = lower.find(sep) {
            cut = cut.min(i);
        }
    }
    // 小写后字节位置可能变化（极少数字符），不在字符边界上时不截
    if !s.is_char_boundary(cut) {
        cut = s.len();
    }
    let s = &s[..cut];
    let first = s.split(['/', '／', '、', '&', '＆', ',', '，', ';', '；', '|']).map(str::trim).find(|p| !p.is_empty()).unwrap_or("");
    let t = squash(first);
    if t.is_empty() { squash(s) } else { t }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_from_real_library() {
        assert_eq!(clean_title("体面 (Live)"), "体面");
        assert_eq!(clean_title("不如回家喝自来水 (Live版)"), "不如回家喝自来水");
        assert_eq!(clean_title("我很好（吉他版）"), "我很好");
        assert_eq!(clean_title("给你呀（又名：for ya）"), "给你呀");
        assert_eq!(clean_title("齐天 (Cover : 大冢爱)"), "齐天");
        assert_eq!(clean_title("亲爱的你的棉裤真的不在我家[music.migu.cn]"), "亲爱的你的棉裤真的不在我家");
        assert_eq!(clean_title("1081.爱情转移【六倍音质】"), "爱情转移");
        assert_eq!(clean_title("漂洋过海来看你【六倍音质】"), "漂洋过海来看你");
        assert_eq!(clean_title("我的热爱  - 伴奏【六倍音质】"), "我的热爱");
        assert_eq!(clean_title("三千世界鸦杀(翻唱)"), "三千世界鸦杀");
        assert_eq!(clean_title("暮色回响 (电影《默杀》推广曲)"), "暮色回响");
    }

    #[test]
    fn titles_left_alone() {
        assert_eq!(clean_title("晴天"), "晴天");
        assert_eq!(clean_title("(Intro)"), "(Intro)");
        assert_eq!(clean_title("1987我不知道会遇见你"), "1987我不知道会遇见你");
        assert_eq!(clean_title("99 Problems"), "99 Problems");
        assert_eq!(clean_title("Sekiro - Enemy alerted sound effect"), "Sekiro - Enemy alerted sound effect");
        assert_eq!(clean_title("we don't talk anymore"), "we don't talk anymore");
        assert_eq!(clean_title("如果我们在余生相遇·巨蟹座"), "如果我们在余生相遇·巨蟹座");
    }

    #[test]
    fn artists() {
        assert_eq!(clean_artist("陈楚生/王赫野"), "陈楚生");
        assert_eq!(clean_artist("齐一[music.migu.cn]"), "齐一");
        assert_eq!(clean_artist("charlie puth&selena gomez"), "charlie puth");
        assert_eq!(clean_artist("6诗人&酱子"), "6诗人");
        assert_eq!(clean_artist("周杰伦 feat. 杨瑞代"), "周杰伦");
        assert_eq!(clean_artist("于文文"), "于文文");
        assert_eq!(clean_artist(""), "");
    }
}
