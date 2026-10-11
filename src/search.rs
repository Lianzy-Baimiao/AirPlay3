//! 曲目搜索：子串匹配 + 拼音首字母匹配。
//!
//! 原版内置一张 65536 项的码位→拼音首字母位图表（资源 `CHINESE`，多音字置多位），
//! 输入 `zjl` 能找到"周杰伦"。这里用 `pinyin` crate 生成同样的位图：
//! 每个汉字一个 token（可能有多个首字母），每个西文单词取首字母作为一个 token，
//! 查询串的每个字母必须依次命中连续的 token。

use pinyin::ToPinyinMulti;

use crate::util::text::to_halfwidth;

/// 位 0..26 = a..z，位 26..36 = 0..9
type Mask = u64;

fn letter_bit(c: char) -> Mask {
    match c {
        'a'..='z' => 1 << (c as u32 - 'a' as u32),
        '0'..='9' => 1 << (26 + c as u32 - '0' as u32),
        _ => 0,
    }
}

/// 单个汉字的首字母位图（多音字取全部读音）。
fn han_mask(c: char) -> Mask {
    let mut m = 0;
    if let Some(multi) = c.to_pinyin_multi() {
        for py in multi {
            if let Some(first) = py.first_letter().chars().next() {
                m |= letter_bit(first);
            }
        }
    }
    m
}

/// 把一个字段拆成 token 位图序列。
fn tokens(field: &str) -> Vec<Mask> {
    let mut out = Vec::new();
    let mut in_word = false;
    for c in field.chars().map(to_halfwidth).flat_map(char::to_lowercase) {
        if c.is_ascii_alphabetic() {
            if !in_word {
                out.push(letter_bit(c));
                in_word = true;
            }
        } else if c.is_ascii_digit() {
            out.push(letter_bit(c));
            in_word = false;
        } else {
            in_word = false;
            let m = han_mask(c);
            if m != 0 {
                out.push(m);
            }
        }
    }
    out
}

/// 预计算的搜索键。
#[derive(Clone, Debug, Default)]
pub struct SearchKey {
    text: String,
    fields: Vec<Vec<Mask>>,
}

impl SearchKey {
    pub fn new(fields: &[&str]) -> SearchKey {
        let text = fields.join("\u{1}").chars().map(to_halfwidth).flat_map(char::to_lowercase).collect();
        SearchKey { text, fields: fields.iter().map(|f| tokens(f)).collect() }
    }
}

/// 解析后的查询。
#[derive(Clone, Debug, Default)]
pub struct Query {
    text: String,
    initials: Option<Vec<Mask>>,
}

impl Query {
    pub fn new(q: &str) -> Query {
        let text: String = q.trim().chars().map(to_halfwidth).flat_map(char::to_lowercase).collect();
        let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let initials = (compact.len() >= 2 && compact.chars().all(|c| c.is_ascii_alphanumeric()))
            .then(|| compact.chars().map(letter_bit).collect());
        Query { text, initials }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn matches(&self, key: &SearchKey) -> bool {
        if self.text.is_empty() || key.text.contains(&self.text) {
            return true;
        }
        let Some(q) = &self.initials else { return false };
        key.fields.iter().any(|f| f.len() >= q.len() && f.windows(q.len()).any(|w| w.iter().zip(q).all(|(t, c)| t & c != 0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(q: &str, fields: &[&str]) -> bool {
        Query::new(q).matches(&SearchKey::new(fields))
    }

    #[test]
    fn substring_and_initials() {
        assert!(m("晴天", &["晴天", "周杰伦", "叶惠美"]));
        assert!(m("zjl", &["晴天", "周杰伦", "叶惠美"]));
        assert!(m("ZJL", &["晴天", "周杰伦", "叶惠美"]));
        assert!(m("jay", &["Sunny Day", "Jay Chou", ""]));
        assert!(m("jc", &["Sunny Day", "Jay Chou", ""]));
        assert!(!m("zjl", &["晴天", "王菲", ""]));
        // 多音字：长 = chang / zhang
        assert!(m("cjjt", &["长江集团", "", ""]));
        assert!(m("zjjt", &["长江集团", "", ""]));
        assert!(m("qyqrq", &["七月七日晴", "许慧欣", ""]));
        // 不跨字段拼接
        assert!(!m("qtz", &["晴天", "周杰伦", ""]));
        assert!(m("", &["任意"]));
    }
}
