//! LRC 歌词解析：多时间标签、`[offset:]`、`[ar:]/[ti:]`、同时间戳的翻译行、逐字时间标签。

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// 秒
    pub time: f64,
    pub text: String,
    /// 同一时间戳的第二行（通常是翻译）
    pub trans: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lyrics {
    pub lines: Vec<Line>,
    /// `[offset:]`，毫秒，正值表示歌词提前
    pub offset_ms: i64,
    pub title: Option<String>,
    pub artist: Option<String>,
    /// 是否带时间轴（否则是纯文本歌词，只能静态显示）
    pub synced: bool,
}

/// 解析 `mm:ss.xx` / `mm:ss:xx` / `mm:ss`。
fn parse_time(tag: &str) -> Option<f64> {
    let (m, rest) = tag.split_once(':')?;
    let m: f64 = m.trim().parse().ok().filter(|v: &f64| *v >= 0.0)?;
    let (s, frac) = match rest.find(['.', ':']) {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, ""),
    };
    let s: f64 = s.trim().parse().ok()?;
    let frac = if frac.is_empty() { 0.0 } else {
        let digits: String = frac.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() { return None; }
        digits.parse::<f64>().ok()? / 10f64.powi(digits.len() as i32)
    };
    Some(m * 60.0 + s + frac)
}

/// 去掉逐字时间标签 `<00:01.23>` 以及行内的 `[00:01.23]`。
fn strip_word_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(['<', '[']) {
        let close = if rest[i..].starts_with('<') { '>' } else { ']' };
        match rest[i..].find(close) {
            Some(j) if parse_time(&rest[i + 1..i + j]).is_some() => {
                out.push_str(&rest[..i]);
                rest = &rest[i + j + 1..];
            }
            _ => {
                out.push_str(&rest[..=i]);
                rest = &rest[i + 1..];
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

pub fn parse(text: &str) -> Lyrics {
    let mut lyr = Lyrics::default();
    let mut timed: Vec<(f64, String)> = Vec::new();
    let mut plain: Vec<String> = Vec::new();
    for raw in text.lines() {
        let mut line = raw.trim().trim_start_matches('\u{feff}');
        let mut times = Vec::new();
        let mut had_meta = false;
        while line.starts_with('[') {
            let Some(end) = line.find(']') else { break };
            let tag = &line[1..end];
            if let Some(t) = parse_time(tag) {
                times.push(t);
            } else if let Some((k, v)) = tag.split_once(':') {
                had_meta = true;
                let v = v.trim();
                match k.trim().to_ascii_lowercase().as_str() {
                    "offset" => lyr.offset_ms = v.parse().unwrap_or(0),
                    "ti" if !v.is_empty() => lyr.title = Some(v.to_string()),
                    "ar" if !v.is_empty() => lyr.artist = Some(v.to_string()),
                    _ => {}
                }
            } else {
                break;
            }
            line = line[end + 1..].trim_start();
        }
        if times.is_empty() {
            if !had_meta && !line.is_empty() {
                plain.push(strip_word_tags(line));
            }
            continue;
        }
        let body = strip_word_tags(line);
        timed.extend(times.into_iter().map(|t| (t, body.clone())));
    }
    if timed.is_empty() {
        lyr.lines = plain.into_iter().map(|text| Line { time: 0.0, text, trans: None }).collect();
        return lyr;
    }
    lyr.synced = true;
    timed.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (time, text) in timed {
        match lyr.lines.last_mut() {
            Some(last) if (last.time - time).abs() < 0.001 && !text.is_empty() => {
                if last.text.is_empty() {
                    last.text = text;
                } else if last.trans.is_none() {
                    last.trans = Some(text);
                }
            }
            _ => lyr.lines.push(Line { time, text, trans: None }),
        }
    }
    lyr
}

impl Lyrics {
    /// 给定播放时间（秒，已含用户偏移）返回当前行下标。
    pub fn index_at(&self, t: f64) -> Option<usize> {
        if !self.synced {
            return None;
        }
        let t = t + self.offset_ms as f64 / 1000.0;
        let n = self.lines.partition_point(|l| l.time <= t);
        n.checked_sub(1)
    }

    /// 当前行内的进度 0..1（用于卡拉 OK 式渐变）。
    pub fn progress_at(&self, idx: usize, t: f64) -> f32 {
        let t = t + self.offset_ms as f64 / 1000.0;
        let start = self.lines[idx].time;
        let end = self.lines.get(idx + 1).map(|l| l.time).unwrap_or(start + 5.0);
        if end <= start { 1.0 } else { ((t - start) / (end - start)).clamp(0.0, 1.0) as f32 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lrc() {
        let src = "[ti:晴天]\n[ar:周杰伦]\n[offset:500]\n[00:01.00][00:10.50]故事的小黄花\n\
                   [00:05.123]从出生那年<00:05.50>就飘着\n[00:05.123]Translation\n[01:02:50]帧格式\n[00:20.00]\n";
        let l = parse(src);
        assert!(l.synced);
        assert_eq!(l.title.as_deref(), Some("晴天"));
        assert_eq!(l.offset_ms, 500);
        let times: Vec<f64> = l.lines.iter().map(|x| x.time).collect();
        assert_eq!(times, vec![1.0, 5.123, 10.5, 20.0, 62.5]);
        assert_eq!(l.lines[1].text, "从出生那年就飘着");
        assert_eq!(l.lines[1].trans.as_deref(), Some("Translation"));
        assert_eq!(l.lines[3].text, "");
        // offset 500ms：0.6s 时已到第一行
        assert_eq!(l.index_at(0.4), None);
        assert_eq!(l.index_at(0.6), Some(0));
        assert_eq!(l.index_at(100.0), Some(4));
    }

    #[test]
    fn plain_text() {
        let l = parse("第一行\n\n第二行 [不是时间]\n");
        assert!(!l.synced);
        assert_eq!(l.lines.len(), 2);
        assert_eq!(l.lines[1].text, "第二行 [不是时间]");
        assert_eq!(l.index_at(3.0), None);
    }
}
