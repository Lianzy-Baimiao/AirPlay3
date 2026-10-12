//! 听网络电台：电台不进曲库，直接把播放地址交给引擎（audio::stream 负责下载和解码）。
//! 上一首 / 下一首在当前电台列表里切换；听本地歌曲时退出电台。

use std::sync::atomic::{AtomicU64, Ordering};

use crate::audio::PlayRequest;
use crate::radio::Station;

use super::Player;
use super::nowplaying::{CoverImage, SharedNow};

/// 电台用的 token 从很大的数开始，与曲目的 token 不冲突（队列查不到它，所以播完不会接下一首）
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1 << 60);

#[derive(Clone, Debug)]
pub struct Radio {
    pub station: Station,
    pub token: u64,
    /// 打开这个电台时所在的列表（上一首 / 下一首用）
    pub list: Vec<Station>,
}

impl Player {
    /// 播放电台；`list` = 电台所在的列表（推荐 / 搜索结果 / 收藏）
    pub fn play_station(&self, st: Station, list: Vec<Station>) {
        let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        let net = {
            let mut g = self.core.lock();
            g.radio = Some(Radio { station: st.clone(), token, list });
            g.settings.radio.last = Some(st.clone());
            g.touch_settings();
            g.settings.network.clone()
        };
        crate::audio::stream::set_network(net.clone());
        self.engine.play(PlayRequest { token, path: st.url.clone().into(), start: 0.0, end: None, seek: 0.0 }, false);
        self.repaint();
    }

    pub fn radio_station(&self) -> Option<Station> {
        self.core.lock().radio.as_ref().map(|r| r.station.clone())
    }

    /// 电台模式下的上一个 / 下一个电台（列表首尾相接）；不在电台模式时返回 false
    pub(super) fn radio_step(&self, forward: bool) -> bool {
        let next = {
            let g = self.core.lock();
            let Some(r) = g.radio.as_ref() else { return false };
            let n = r.list.len();
            let i = r.list.iter().position(|s| s.url == r.station.url);
            match (n, i) {
                (0, _) => None,
                (_, None) => Some((r.list[0].clone(), r.list.clone())),
                (_, Some(i)) => {
                    let j = if forward { (i + 1) % n } else { (i + n - 1) % n };
                    Some((r.list[j].clone(), r.list.clone()))
                }
            }
        };
        if let Some((st, list)) = next {
            self.play_station(st, list);
        }
        true
    }

    /// 电台模式下按「播放」：重新连接当前电台
    pub(super) fn radio_resume(&self) -> bool {
        let r = self.core.lock().radio.clone();
        match r {
            Some(r) => {
                self.play_station(r.station, r.list);
                true
            }
            None => false,
        }
    }

    /// 后台下载电台图标，作为「专辑图片」显示
    pub fn load_station_icon(&self, st: &Station, now: &SharedNow) {
        if st.favicon.is_empty() {
            return;
        }
        let (url, name, net, now, p) = (st.favicon.clone(), st.name.clone(), self.core.lock().settings.network.clone(), now.clone(), self.clone());
        std::thread::spawn(move || {
            let Ok(agent) = crate::lyrics::lrclib::agent(&net) else { return };
            let Ok(bytes) = crate::media::artwork::download(&agent, &url) else { return };
            let Some((w, h, rgba)) = crate::media::cover::decode_rgba(&bytes, 640) else { return };
            let mut n = now.lock();
            // 还在听这个电台才显示
            if n.track.is_none() && n.title == name {
                n.cover = Some(CoverImage { size: [w as usize, h as usize], rgba: std::sync::Arc::new(rgba), raw: std::sync::Arc::new(bytes) });
                n.rev += 1;
                drop(n);
                p.repaint();
            }
        });
    }
}
