//! 关于页：项目入口和用户主动触发的更新检查。

use std::cmp::Ordering;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use super::app::App;
use super::panels::settings::State;
use super::settings_layout::{self as lay, Btn, Item, Toggle, Widget};
use super::settings_model::TAB_ABOUT;
use crate::updates::{self, Release};

#[derive(Default)]
pub enum UpdateState {
    #[default]
    Idle,
    Checking(u64),
    Ready(Release),
    Failed(String),
}

impl UpdateState {
    fn begin(&mut self) -> Option<u64> {
        if matches!(self, Self::Checking(_)) { return None; }
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let seq = NEXT.fetch_add(1, AtomicOrdering::Relaxed);
        *self = Self::Checking(seq);
        Some(seq)
    }

    fn complete(&mut self, seq: u64, result: Result<Release, String>) -> bool {
        if !matches!(self, Self::Checking(current) if *current == seq) { return false; }
        *self = match result { Ok(r) => Self::Ready(r), Err(e) => Self::Failed(e) };
        true
    }

    pub fn release_url(&self) -> &str {
        match self { Self::Ready(r) => &r.url, _ => updates::RELEASES_URL }
    }

    fn message(&self) -> String {
        match self {
            Self::Idle => "尚未检查更新\n点击按钮查询 GitHub 的最新正式版。".into(),
            Self::Checking(_) => "正在连接 GitHub…\n你可以继续使用播放器。".into(),
            Self::Ready(r) => match r.comparison {
                Ordering::Greater => format!("发现新版本 {}\n点击「查看发行版」阅读说明并自行下载。", r.tag),
                Ordering::Equal => format!("当前已是最新正式版\nGitHub 最新版本：{}", r.tag),
                Ordering::Less => format!("当前版本高于 GitHub 最新正式版\nGitHub 最新版本：{}", r.tag),
            },
            Self::Failed(e) => format!("{e}\n也可以点击「查看发行版」在浏览器中查看。"),
        }
    }
}

pub fn items(state: &UpdateState) -> Vec<(Item, State)> {
    let mut text_index = 0;
    lay::layout(TAB_ABOUT).into_iter().map(|it| {
        let mut st = State { enabled: true, ..Default::default() };
        match &it.w {
            Widget::Text => {
                st.value = if text_index == 0 { format!("当前版本：v{}  ·  Windows x64", updates::VERSION) } else { state.message() };
                text_index += 1;
            }
            Widget::Pill { act: Toggle::Act(Btn::CheckUpdates), .. } => {
                st.enabled = !matches!(state, UpdateState::Checking(_));
                st.dim = !st.enabled;
                st.value = if st.enabled { "检查更新" } else { "正在检查…" }.into();
            }
            _ => {}
        }
        (it, st)
    }).collect()
}

impl App {
    pub(super) fn check_updates(&mut self) {
        let Some(sw) = self.settings_win.as_mut() else { return };
        let Some(seq) = sw.update.begin() else { return };
        sw.win.dirty = true;
        let net = self.player.core.lock().settings.network.clone();
        let (tx, hwnd) = (self.async_tx.clone(), self.bar_hwnd().0 as isize);
        std::thread::spawn(move || {
            use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
            use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
            let result = updates::check(&net);
            let _ = tx.send(super::actions::AsyncMsg::UpdateChecked(seq, result));
            unsafe { let _ = PostMessageW(Some(HWND(hwnd as _)), super::app::WM_APP_ASYNC, WPARAM(0), LPARAM(0)); }
        });
    }

    pub(super) fn on_update_checked(&mut self, seq: u64, result: Result<Release, String>) {
        if let Some(sw) = self.settings_win.as_mut() {
            if sw.update.complete(seq, result) {
                sw.win.dirty = true;
                self.paint_settings();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_is_manual_single_flight_and_ignores_stale_results() {
        let mut state = UpdateState::default();
        assert!(matches!(state, UpdateState::Idle));
        assert!(items(&state).iter().any(|(_, s)| s.value.contains("尚未检查")));
        let first = state.begin().unwrap();
        assert!(state.begin().is_none(), "连续点击不能发起重复请求");
        assert!(items(&state).iter().any(|(it, s)| matches!(it.w, Widget::Pill { act: Toggle::Act(Btn::CheckUpdates), .. }) && !s.enabled));
        assert!(!state.complete(first + 1, Err("过期结果".into())));
        assert!(state.complete(first, Err("连接失败".into())));
        assert!(state.message().contains("连接失败"));
        assert_eq!(state.release_url(), updates::RELEASES_URL);
        let retry = state.begin().unwrap();
        assert_ne!(retry, first);
        assert!(!state.complete(first, Err("过期结果".into())));
        // 关闭后重新打开配置窗口，即使旧请求尚未结束，也不能覆盖新窗口的状态。
        let mut reopened = UpdateState::default();
        let new_seq = reopened.begin().unwrap();
        assert!(!reopened.complete(retry, Err("旧窗口结果".into())));
        assert!(reopened.complete(new_seq, Ok(Release { tag: "v9.0.0".into(), comparison: Ordering::Greater, url: format!("{}/releases/tag/v9.0.0", updates::PROJECT_URL) })));
        assert!(reopened.message().contains("发现新版本 v9.0.0"));
        assert!(reopened.release_url().ends_with("/releases/tag/v9.0.0"));
    }
}
