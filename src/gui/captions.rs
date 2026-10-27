//! 电台字幕：听网络电台时用 Windows 11 的「实时辅助字幕」识别正在播的声音（说话、唱词），
//! 显示在歌词的位置上：歌词图片面板、桌面歌词、迷你歌词条、任务栏歌词。
//! 识别结果是几段不断变长、还会回头改字的文字；各处按自己的宽度换行，只显示最后几行。
//! 一阵没有新的字就收起，回到电台名和节目。

use std::time::{Duration, Instant};

use super::app::{App, Pane};
use super::text::{self, Font};
use crate::platform::livecaptions::Status;

/// 多久没有新的字就收起字幕
const IDLE: Duration = Duration::from_secs(12);
/// 出错提示显示多久
const NOTICE: Duration = Duration::from_secs(6);
/// 暂停（或缓冲）多久以后才关掉识别：电台重新连接、短暂暂停时不必重启它
const LINGER: Duration = Duration::from_secs(8);
/// 换台后多久内不认新的字：旧电台的最后一句它还会改几次
const GRACE: Duration = Duration::from_millis(1500);

#[derive(Default)]
pub struct CaptionView {
    /// 要显示的几段（越后越新）；空 = 现在不显示字幕
    pub paras: Vec<String>,
    /// paras 是一条提示（出错了），不是识别结果
    pub notice: bool,
    /// paras 变了就 +1
    pub rev: u64,
    /// 到这个时刻收起
    until: Option<Instant>,
    /// 识别状态看到哪一版了
    seen: u64,
    /// 正在给哪个电台显示字幕（没开字幕时为 None）
    station: Option<String>,
    /// 换台的时刻（GRACE 以内不显示新的字）
    switched: Option<Instant>,
    /// 已经不再显示的段（换台前的、收起前的）
    old: Vec<String>,
    /// 识别的最新结果（几段）
    src: Vec<String>,
    /// 开始暂停的时刻
    paused: Option<Instant>,
}

impl CaptionView {
    fn show(&mut self, paras: Vec<String>, notice: bool, until: Option<Instant>) {
        if paras != self.paras || notice != self.notice {
            (self.paras, self.notice) = (paras, notice);
            self.rev += 1;
        }
        self.until = until;
    }
}

impl App {
    /// 菜单「电台字幕」：打开 / 关闭（结果写在电台列表下面）
    pub fn toggle_captions(&mut self) {
        let on = {
            let mut g = self.player.core.lock();
            g.settings.radio.captions = !g.settings.radio.captions;
            g.touch_settings();
            g.settings.radio.captions
        };
        self.player.save(false);
        let note = match crate::platform::livecaptions::check() {
            Err(e) if on => format!("字幕：{e}"),
            _ if on => "电台字幕已打开".into(),
            _ => "电台字幕已关闭".into(),
        };
        self.radio_note(note);
    }

    /// 每帧：按开关和播放状态开 / 关识别，取识别结果，决定显示哪几段
    pub fn sync_captions(&mut self) {
        let now = Instant::now();
        let (on, enabled) = {
            let g = self.player.core.lock();
            (g.settings.radio.captions, g.settings.radio.enabled)
        };
        let station = self.ui.snap.radio.clone().filter(|_| on && enabled && !self.quitting);
        let rev0 = self.ui.caption.rev;
        let cv = &mut self.ui.caption;
        // 换台（或开 / 关字幕）：先不显示，旧电台的字不再出来；上次出错停了的话重新试
        if station != cv.station {
            cv.station = station.clone();
            cv.switched = Some(now);
            cv.show(Vec::new(), false, None);
            if station.is_some() {
                self.captions.retry();
            }
        }
        if station.is_some() && !self.ui.snap.playing {
            cv.paused.get_or_insert(now);
        } else {
            cv.paused = None;
        }
        let want = station.is_some() && cv.paused.is_none_or(|t| now - t < LINGER);
        self.captions.set(want);
        let mut fresh = false;
        if let Some(s) = self.captions.changed(cv.seen) {
            cv.seen = s.rev;
            match s.status {
                Status::Running => {
                    cv.src = s.text.split('\n').filter(|p| !p.is_empty()).map(str::to_string).collect();
                    fresh = true;
                }
                Status::Failed(e) => {
                    cv.src.clear();
                    if want {
                        let msg = format!("字幕：{e}");
                        // 歌词图片面板：写在顶部的说明文字那里（显示几秒）
                        self.ui.cover_status = Some((msg.clone(), now));
                        cv.show(vec![msg], true, Some(now + NOTICE));
                    }
                }
                Status::Off | Status::Starting => cv.src.clear(),
            }
        }
        if cv.switched.is_some_and(|t| now - t >= GRACE) {
            cv.switched = None;
            cv.old = cv.src.clone();
        }
        if fresh && cv.switched.is_none() {
            cv.old.retain(|p| cv.src.contains(p));
            let show: Vec<String> = cv.src.iter().filter(|p| !cv.old.contains(p)).cloned().collect();
            if !show.is_empty() && (show != cv.paras || cv.notice) {
                cv.show(show, false, Some(now + IDLE));
            }
        }
        // 一阵没有新的字：收起，回到电台名和节目；再说话时只显示新的
        if cv.until.is_some_and(|t| now >= t) {
            cv.old = cv.src.clone();
            cv.show(Vec::new(), false, None);
        }
        if self.ui.caption.rev != rev0 {
            self.dirty(Pane::Cover);
        }
    }
}

/// 一个字的宽度（逻辑像素）
fn char_w(ch: char, font: Font) -> f32 {
    let mut b = [0u8; 4];
    text::measure(ch.encode_utf8(&mut b), font).0
}

/// 按宽度换行：中文每个字之间都能断，英文、数字整词不断开，标点不放在行首
pub fn wrap(s: &str, font: Font, max: f32) -> Vec<String> {
    wrap_with(s, max, |c| char_w(c, font))
}

/// 末尾放得下 `max` 宽的一段（字幕只显示一行时用：最新的字在最右边）
pub fn tail_fit(s: &str, font: Font, max: f32) -> &str {
    tail_fit_with(s, max, |c| char_w(c, font))
}

fn tail_fit_with(s: &str, max: f32, cw: impl Fn(char) -> f32) -> &str {
    let mut w = 0.0;
    for (i, ch) in s.char_indices().rev() {
        w += cw(ch);
        if w > max {
            return &s[i + ch.len_utf8()..];
        }
    }
    s
}

/// 不能放在行首的标点
fn no_line_start(c: char) -> bool {
    "，。、！？；：,.!?;:）)」』》】〉”’…—～·%".contains(c)
}

/// 断行的单位：英文单词、数字（含 9.5、80%）各是一个，其余每个字一个
fn units(s: &str) -> Vec<&str> {
    let mut v = Vec::new();
    let mut word: Option<usize> = None;
    for (i, c) in s.char_indices() {
        let in_word = c.is_ascii_alphanumeric() || (word.is_some() && "'.-%".contains(c));
        match (in_word, word) {
            (true, None) => word = Some(i),
            (true, Some(_)) => {}
            (false, w) => {
                if let Some(st) = w {
                    v.push(&s[st..i]);
                    word = None;
                }
                v.push(&s[i..i + c.len_utf8()]);
            }
        }
    }
    if let Some(st) = word {
        v.push(&s[st..]);
    }
    v
}

fn wrap_with(s: &str, max: f32, cw: impl Fn(char) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    let (mut line, mut w) = (String::new(), 0.0f32);
    for u in units(s) {
        let uw: f32 = u.chars().map(&cw).sum();
        let space = u.starts_with(char::is_whitespace);
        if w + uw > max && !line.is_empty() && !u.starts_with(no_line_start) {
            lines.push(std::mem::take(&mut line).trim_end().to_string());
            w = 0.0;
        }
        // 行首不放空格
        if line.is_empty() && space {
            continue;
        }
        line.push_str(u);
        w += uw;
    }
    let line = line.trim_end();
    if !line.is_empty() {
        lines.push(line.to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的宽度：中文 1，英文、数字、空格 0.5
    fn cw(c: char) -> f32 {
        if c.is_ascii() { 0.5 } else { 1.0 }
    }

    #[test]
    fn wraps_chinese_anywhere_but_not_before_punctuation() {
        assert_eq!(wrap_with("今天天气很好，我们去公园", 5.0, cw), ["今天天气很", "好，我们去", "公园"]);
        // 逗号不放到行首：留在上一行（稍微超出一点）
        assert_eq!(wrap_with("一二三四五，六", 5.0, cw), ["一二三四五，", "六"]);
        assert_eq!(wrap_with("", 5.0, cw), Vec::<String>::new());
    }

    #[test]
    fn keeps_words_and_numbers_whole() {
        assert_eq!(wrap_with("hello world foo", 3.0, cw), ["hello", "world", "foo"]);
        assert_eq!(wrap_with("早上9.5点至下午5点", 5.0, cw), ["早上9.5点", "至下午5点"]);
        assert_eq!(units("AI是80%的"), ["AI", "是", "80%", "的"]);
    }

    #[test]
    fn tail_keeps_the_newest_characters() {
        assert_eq!(tail_fit_with("一二三四五六", 3.0, cw), "四五六");
        assert_eq!(tail_fit_with("一二三", 3.0, cw), "一二三");
        assert_eq!(tail_fit_with("abc一二", 3.0, cw), "bc一二");
    }
}
