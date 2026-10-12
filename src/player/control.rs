//! 播放操作：播放、暂停、上一首/下一首、跳转、音量、模式、喜爱/不再播放、均衡器、设备。

use crate::audio::PlayState;
use crate::config::{AudioSettings, EqSettings, PlayMode};
use crate::library::{ListKind, TrackId};

use super::{Player, Source};

impl Player {
    /// 从 `src` 播放 `id`。
    pub fn play_at(&self, src: Source, id: TrackId, seek: f64, paused: bool) {
        let mut g = self.core.lock();
        let core = &mut *g;
        // 听本地歌曲：退出电台
        core.radio = None;
        if let Some(t) = core.lib.tracks.get_mut(&id).filter(|t| t.broken) {
            t.broken = false; // 用户主动播放时重新尝试
        }
        core.queue.set_source(src);
        core.queue.set_current(&core.lib, id);
        if let Some(req) = core.queue.request(&core.lib, id, seek) {
            self.engine.play(req, paused);
        }
        drop(g);
        self.repaint();
    }

    pub fn toggle_pause(&self) {
        match self.engine.state() {
            PlayState::Playing => self.engine.set_paused(true),
            PlayState::Paused => self.engine.set_paused(false),
            PlayState::Stopped => {
                if !self.radio_resume() {
                    self.play_current_or_first();
                }
            }
        }
        self.repaint();
    }

    pub fn play(&self) {
        match self.engine.state() {
            PlayState::Paused => self.engine.set_paused(false),
            PlayState::Stopped => {
                if !self.radio_resume() {
                    self.play_current_or_first();
                }
            }
            PlayState::Playing => {}
        }
        self.repaint();
    }

    pub fn pause(&self) {
        if self.engine.state() == PlayState::Playing {
            self.engine.set_paused(true);
        }
        self.repaint();
    }

    fn play_current_or_first(&self) {
        let g = self.core.lock();
        let src = g.queue.source.clone().unwrap_or(Source::List(g.lib.special_id(ListKind::Local)));
        let id = g.queue.current.filter(|id| g.lib.track(*id).is_some());
        let first = || {
            let mut q = super::Queue::default();
            q.set_source(src.clone());
            q.first_playable(&g.lib)
        };
        let Some(id) = id.or_else(first) else { return };
        drop(g);
        self.play_at(src, id, 0.0, false);
    }

    pub fn next(&self) {
        self.step(true);
    }

    pub fn prev(&self) {
        self.step(false);
    }

    fn step(&self, forward: bool) {
        // 听电台时：上一个 / 下一个电台
        if self.radio_step(forward) {
            return;
        }
        let mut g = self.core.lock();
        let Some(cur) = self.current(&g) else {
            drop(g);
            return self.play_current_or_first();
        };
        let core = &mut *g;
        let mode = core.settings.playback.mode;
        let id = if forward { core.queue.next_after(&core.lib, cur, mode, false) } else { core.queue.prev_before(&core.lib, cur, mode) };
        let Some(id) = id else { return };
        core.queue.set_current(&core.lib, id);
        if let Some(req) = core.queue.request(&core.lib, id, 0.0) {
            self.engine.play(req, false);
        }
        drop(g);
        self.repaint();
    }

    pub fn stop(&self) {
        self.engine.stop();
        self.repaint();
    }

    pub fn seek(&self, secs: f64) {
        // 网络电台是直播，不能跳转
        if self.core.lock().radio.is_some() {
            return;
        }
        self.engine.seek(secs);
        self.repaint();
    }

    pub fn set_volume(&self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        self.engine.set_volume(v);
        let mut g = self.core.lock();
        g.settings.playback.volume = v;
        if g.settings.playback.muted {
            g.settings.playback.muted = false;
            self.engine.set_muted(false);
        }
        g.touch_settings();
        drop(g);
        self.repaint();
    }

    pub fn volume_step(&self, delta: f32) {
        let v = self.core.lock().settings.playback.volume;
        self.set_volume(v + delta);
    }

    pub fn toggle_mute(&self) {
        let mut g = self.core.lock();
        let m = !g.settings.playback.muted;
        g.settings.playback.muted = m;
        g.touch_settings();
        drop(g);
        self.engine.set_muted(m);
        self.repaint();
    }

    pub fn set_mode(&self, mode: PlayMode) {
        let mut g = self.core.lock();
        g.settings.playback.mode = mode;
        g.touch_settings();
        drop(g);
        self.repaint();
    }

    /// 切换当前曲目的"喜爱"，返回新状态。
    pub fn toggle_favorite_current(&self) -> Option<bool> {
        let mut g = self.core.lock();
        let id = self.current(&g)?;
        let on = g.lib.toggle_favorite(id);
        drop(g);
        self.repaint();
        Some(on)
    }

    /// 当前曲目加入"不再播放"并切到下一首。
    pub fn ban_current(&self) {
        let mut g = self.core.lock();
        let Some(id) = self.current(&g) else { return };
        g.lib.set_banned(id, true);
        drop(g);
        self.next();
    }

    pub fn set_eq(&self, eq: &EqSettings) {
        self.engine.set_eq(eq);
        let mut g = self.core.lock();
        g.settings.eq = eq.clone();
        g.touch_settings();
    }

    pub fn set_audio(&self, a: &AudioSettings) {
        let mut g = self.core.lock();
        if g.settings.audio != *a {
            self.engine.set_audio(a);
        }
        g.settings.audio = a.clone();
        g.touch_settings();
    }
}
