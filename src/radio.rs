//! 网络电台：只认用户自己粘贴进来的电台清单。程序里没有任何电台地址，也不提供、不整理电台目录；
//! 这里只负责把清单（M3U / EXTM3U、PLS、「名字,地址」的文本）解析成电台，播放交给 audio::stream。

use serde::{Deserialize, Serialize};

/// 一个电台（存在配置里）
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Station {
    pub name: String,
    /// 播放地址（http / https 的直播流或 HLS 播放列表）
    pub url: String,
    /// 清单里写的分组（EXTM3U 的 group-title、文本清单的「分组,#genre#」）；导入时按它分到不同的清单
    #[serde(alias = "country")]
    pub group: String,
    /// 清单里给的台标地址（EXTM3U 的 tvg-logo），作为「专辑图片」显示
    #[serde(alias = "favicon")]
    pub logo: String,
}

impl Station {
    /// 列表里名字后面显示的说明：地址的主机名
    pub fn host(&self) -> &str {
        host_of(&self.url)
    }
}

/// 一份电台清单（用户粘贴进来的一组电台，显示为列表左边的一栏）
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StationList {
    pub name: String,
    pub stations: Vec<Station>,
}

fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

/// 一行里 http:// 或 https:// 开始的位置
fn find_url(line: &str) -> Option<usize> {
    let lower = line.to_ascii_lowercase();
    [lower.find("http://"), lower.find("https://")].into_iter().flatten().min()
}

/// 地址到第一个空白为止
fn url_at(line: &str, at: usize) -> String {
    line[at..].split_whitespace().next().unwrap_or("").to_string()
}

fn station(name: &str, url: String, group: &str, logo: &str) -> Station {
    let name = name.trim();
    let name = if name.is_empty() { host_of(&url).to_string() } else { name.to_string() };
    Station { name, url, group: group.trim().to_string(), logo: logo.trim().to_string() }
}

/// 解析粘贴进来的清单：EXTM3U（#EXTINF 的 tvg-logo、group-title）、PLS（File1= / Title1=）、
/// 或每行「名字,地址」「名字 地址」「地址」（「分组,#genre#」这一行开始一个分组）
pub fn parse_list(text: &str) -> Vec<Station> {
    let text = text.trim_start_matches('\u{feff}');
    let lower = text.to_ascii_lowercase();
    if lower.contains("[playlist]") || lower.lines().any(|l| l.trim_start().starts_with("file1=")) {
        return parse_pls(text);
    }
    if lower.contains("#extm3u") || lower.contains("#extinf") {
        return parse_m3u(text);
    }
    parse_text(text)
}

fn parse_pls(text: &str) -> Vec<Station> {
    let mut files = std::collections::BTreeMap::<u32, (String, String)>::new();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let k = k.trim().to_ascii_lowercase();
        let (kind, n) = if let Some(n) = k.strip_prefix("file") {
            (0, n)
        } else if let Some(n) = k.strip_prefix("title") {
            (1, n)
        } else {
            continue;
        };
        let Ok(n) = n.parse::<u32>() else { continue };
        let e = files.entry(n).or_default();
        if kind == 0 { e.0 = v.trim().to_string() } else { e.1 = v.trim().to_string() }
    }
    files.into_values().filter(|(u, _)| find_url(u) == Some(0)).map(|(u, t)| station(&t, u, "", "")).collect()
}

/// #EXTINF 一行里的属性值（如 tvg-logo="..."）
fn attr(line: &str, key: &str) -> String {
    let lower = line.to_ascii_lowercase();
    let Some(i) = lower.find(&format!("{key}=\"")) else { return String::new() };
    let rest = &line[i + key.len() + 2..];
    rest.split('"').next().unwrap_or("").to_string()
}

/// #EXTINF 的标题：引号外面第一个逗号后面的部分
fn extinf_title(line: &str) -> &str {
    let mut quoted = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            ',' if !quoted => return &line[i + 1..],
            _ => {}
        }
    }
    ""
}

fn parse_m3u(text: &str) -> Vec<Station> {
    let mut out = Vec::new();
    let (mut name, mut group, mut logo, mut grp) = (String::new(), String::new(), String::new(), String::new());
    for line in text.lines().map(str::trim) {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("#extinf") {
            name = extinf_title(line).trim().to_string();
            group = attr(line, "group-title");
            logo = attr(line, "tvg-logo");
        } else if lower.starts_with("#extgrp:") {
            grp = line[8..].trim().to_string();
        } else if line.starts_with('#') || line.is_empty() {
            continue;
        } else if find_url(line) == Some(0) {
            let g = if group.is_empty() { grp.as_str() } else { group.as_str() };
            out.push(station(&name, url_at(line, 0), g, &logo));
            (name, group, logo) = (String::new(), String::new(), String::new());
        }
    }
    out
}

fn parse_text(text: &str) -> Vec<Station> {
    let mut out = Vec::new();
    let mut group = String::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        match find_url(line) {
            Some(at) => {
                // 名字和地址之间的逗号、竖线、制表符
                let name = line[..at].trim_end().trim_end_matches([',', '，', '|', '\t']);
                out.push(station(name, url_at(line, at), &group, ""));
            }
            // 「分组名,#genre#」：后面的电台都放进这个分组
            None => {
                if let Some(g) = line.strip_suffix("#genre#") {
                    group = g.trim_end().trim_end_matches([',', '，']).trim().to_string();
                }
            }
        }
    }
    out
}

/// 把解析出的电台并进已有的清单：有分组的放进同名清单（没有就新建），没有分组的放进一个新的「清单 N」；
/// 已经有的地址不重复添加。返回（新加了几个电台，第一个加了电台的清单的序号）
pub fn import(lists: &mut Vec<StationList>, stations: Vec<Station>) -> (usize, Option<usize>) {
    let (mut added, mut first) = (0, None);
    let mut loose = Vec::new();
    for st in stations {
        if st.group.is_empty() {
            // 没有分组的：所有清单里都没有这个地址才加
            if !lists.iter().any(|l| l.stations.iter().any(|s| s.url == st.url)) && !loose.iter().any(|s: &Station| s.url == st.url) {
                loose.push(st);
            }
            continue;
        }
        let i = match lists.iter().position(|l| l.name == st.group) {
            Some(i) => i,
            None => {
                lists.push(StationList { name: st.group.clone(), stations: Vec::new() });
                lists.len() - 1
            }
        };
        if !lists[i].stations.iter().any(|s| s.url == st.url) {
            lists[i].stations.push(st);
            added += 1;
            first.get_or_insert(i);
        }
    }
    if !loose.is_empty() {
        let name = (1..).map(|n| format!("清单 {n}")).find(|n| !lists.iter().any(|l| &l.name == n)).unwrap_or_default();
        added += loose.len();
        lists.push(StationList { name, stations: loose });
        first.get_or_insert(lists.len() - 1);
    }
    // 新建了却没加进电台的分组（全是重复的）去掉
    lists.retain(|l| !l.stations.is_empty());
    (added, first.map(|i| i.min(lists.len().saturating_sub(1))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m3u() {
        let s = parse_list(
            "#EXTM3U\n#EXTINF:-1 tvg-logo=\"https://img.example.com/a.png\" group-title=\"新闻, 资讯\",电台 A\nhttps://a.example.com/live.m3u8\n\
             #EXTINF:-1,电台 B\n#EXTGRP:音乐\nhttp://b.example.com:8000/stream\n#EXTINF:-1,\nhttps://c.example.com/x.mp3\n",
        );
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].name.as_str(), s[0].group.as_str(), s[0].logo.as_str()), ("电台 A", "新闻, 资讯", "https://img.example.com/a.png"));
        assert_eq!((s[1].name.as_str(), s[1].group.as_str(), s[1].url.as_str()), ("电台 B", "音乐", "http://b.example.com:8000/stream"));
        assert_eq!((s[2].name.as_str(), s[2].host()), ("c.example.com", "c.example.com"));
    }

    #[test]
    fn plain_and_pls() {
        let s = parse_list("新闻,#genre#\n电台 A,https://a.example.com/1.mp3\n电台 B，http://b.example.com/2\n随便一行\n音乐,#genre#\nhttps://c.example.com/3 \n电台 D | rtmp://d.example.com\n");
        assert_eq!(s.iter().map(|s| (s.name.as_str(), s.group.as_str())).collect::<Vec<_>>(), [("电台 A", "新闻"), ("电台 B", "新闻"), ("c.example.com", "音乐")]);
        let p = parse_list("[playlist]\nFile2=https://b.example.com/2\nTitle1=电台 A\nFile1=http://a.example.com/1\nNumberOfEntries=2\n");
        assert_eq!(p.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["电台 A", "b.example.com"]);
    }

    #[test]
    fn import_merges_and_skips_duplicates() {
        let mut lists = Vec::new();
        let text = "电台 A,https://a.example.com/1\n新闻,#genre#\n电台 B,https://b.example.com/2\n";
        assert_eq!(import(&mut lists, parse_list(text)), (2, Some(0)));
        assert_eq!(lists.iter().map(|l| (l.name.as_str(), l.stations.len())).collect::<Vec<_>>(), [("新闻", 1), ("清单 1", 1)]);
        // 再粘贴一次：什么都不加，也不多出空清单
        assert_eq!(import(&mut lists, parse_list(text)), (0, None));
        assert_eq!(lists.len(), 2);
        let (n, first) = import(&mut lists, parse_list("电台 C,https://c.example.com/3"));
        assert_eq!((n, first, lists[2].name.as_str()), (1, Some(2), "清单 2"));
    }
}
