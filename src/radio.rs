//! 网络电台：电台信息（收藏时存进配置）。没有电台目录，`top` / `search` 只返回错误。

use serde::{Deserialize, Serialize};

use crate::config::NetworkSettings;

/// 一个电台（收藏时存进配置）
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Station {
    pub uuid: String,
    pub name: String,
    /// 播放地址
    pub url: String,
    pub homepage: String,
    pub favicon: String,
    pub country: String,
    pub tags: String,
    pub codec: String,
    pub bitrate: u32,
}

impl Station {
    /// 列表里第二行的说明：国家 · 标签
    pub fn detail(&self) -> String {
        let tags: Vec<&str> = self.tags.split(',').map(str::trim).filter(|t| !t.is_empty()).take(3).collect();
        [self.country.as_str(), &tags.join(" / ")].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ")
    }

    /// 码流：「MP3 128k」「AAC」
    pub fn format(&self) -> String {
        let c = if self.codec.eq_ignore_ascii_case("unknown") { "" } else { self.codec.as_str() };
        match (c.is_empty(), self.bitrate) {
            (true, 0) => String::new(),
            (true, b) => format!("{b}k"),
            (false, 0) => c.to_uppercase(),
            (false, b) => format!("{} {b}k", c.to_uppercase()),
        }
    }
}

const REMOVED: &str = "没有电台目录";

/// 按地区列出热门电台（没有电台目录）
pub fn top(_net: &NetworkSettings, _country: &str, _limit: u32) -> Result<Vec<Station>, String> {
    Err(REMOVED.into())
}

/// 按名字搜索电台（没有电台目录）
pub fn search(_net: &NetworkSettings, _name: &str, _country: &str) -> Result<Vec<Station>, String> {
    Err(REMOVED.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn station_text() {
        let s = Station { name: "电台".into(), country: "China".into(), tags: "news,talk,mandarin,extra".into(), codec: "MP3".into(), bitrate: 64, ..Default::default() };
        assert_eq!(s.detail(), "China · news / talk / mandarin");
        assert_eq!(s.format(), "MP3 64k");
        assert_eq!(Station { codec: "UNKNOWN".into(), ..Default::default() }.format(), "");
    }
}
