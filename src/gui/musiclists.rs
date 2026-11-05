//! 本地播放列表的显示、选择与管理。绘制和点击共用同一份分栏顺序。

use super::app::App;
use super::menu::{Act, Item, item};
use crate::library::{Library, ListId, ListKind};

pub(super) fn visible_lists(lib: &Library, selected: Option<ListId>) -> Vec<(ListId, String)> {
    lib.lists.iter()
        .filter(|l| matches!(l.kind, ListKind::Audition | ListKind::User) || !l.tracks.is_empty() || selected == Some(l.id))
        .map(|l| (l.id, if l.kind == ListKind::Audition { "默认".into() } else { l.name.clone() }))
        .collect()
}

impl App {
    pub(super) fn music_lists(&self) -> Vec<(ListId, String)> {
        visible_lists(&self.player.core.lock().lib, self.ui.list.list)
    }

    pub(super) fn select_music_list(&mut self, id: ListId) {
        if self.player.core.lock().lib.list(id).is_none() {
            return;
        }
        self.set_search_focus(false);
        let l = &mut self.ui.list;
        l.list = Some(id);
        l.search.clear();
        l.selected.clear();
        l.ids.clear();
        l.ids_key = None;
        l.scroll = 0;
        l.hover = None;
        l.anchor = None;
        l.reveal_tab = true;
        self.refresh_list();
    }

    pub(super) fn music_list_at(&self, x: i32, y: i32) -> Option<ListId> {
        let lay = self.list_layout();
        if !lay.tabs.contains(x, y) {
            return None;
        }
        let tabs = self.music_lists();
        lay.tab_at(y, self.ui.list.tab_scroll, tabs.len()).map(|i| tabs[i].0)
    }

    pub(super) fn music_list_menu(&self) -> Vec<Item> {
        let mut items = vec![item("新建播放列表…", Act::NewList), item("文件夹新建为播放列表…", Act::FolderList)];
        let g = self.player.core.lock();
        if let Some(l) = self.ui.list.list.and_then(|id| g.lib.list(id)) {
            items.extend([Item::Sep, item("添加文件", Act::AddFiles), item("添加文件夹", Act::AddFolder)]);
            if matches!(l.kind, ListKind::User | ListKind::Local) {
                items.push(item("重命名播放列表…", Act::RenameList));
            }
        }
        items
    }

    pub(super) fn prompt_list_name(&mut self, rename: bool) {
        let (id, initial) = {
            let g = self.player.core.lock();
            if rename {
                let Some(l) = self.ui.list.list.and_then(|id| g.lib.list(id))
                    .filter(|l| matches!(l.kind, ListKind::User | ListKind::Local)) else { return };
                (Some(l.id), l.name.clone())
            } else {
                let mut name = "新建列表".to_string();
                let mut n = 2;
                while g.lib.lists.iter().any(|l| l.name == name) {
                    name = format!("新建列表 {n}");
                    n += 1;
                }
                (None, name)
            }
        };
        self.open_list_name(id, &initial);
    }

    pub(super) fn finish_list_name(&mut self, id: Option<ListId>, name: &str) {
        if name.trim().is_empty() {
            return;
        }
        let id = {
            let mut g = self.player.core.lock();
            match id {
                Some(id) if g.lib.rename_list(id, name) => id,
                Some(_) => return,
                None => g.lib.create_list(name),
            }
        };
        self.show_radio_tab(false);
        self.select_music_list(id);
        self.player.save(false);
    }
}
