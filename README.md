<p align="center"><img src="assets/icon_256.png" width="96" alt="AirPlay3"></p>

<h1 align="center">AirPlay3 · Rust 重写版</h1>

<p align="center">一款早已停更的经典本地音乐播放器，用 Rust 让它复活。</p>

> **这是我「经典软件回归计划」的第一步。**
> 计划把那些陪伴过我们、但早已停止维护的 Windows 经典小软件，逐个用现代技术重写，让它们在今天的系统上继续好好用下去。

## 致敬 AirPlay3

AirPlay3 是 2012 年前后的一款国产绿色音乐播放器（airplayme.com）。它体积小巧、无需安装，深色极简的界面由几块磁吸在一起的半透明窗口组成：播放条、歌词图片、均衡器、音乐列表。它的标语是 **「乐声飞扬 聆听彼此」 / 「link between souls」**。

原作者早已停止更新，它依赖的服务器也已全部关闭，但它的界面至今仍让人怀念。本项目没有原版源码，全部功能是对原版程序逆向分析后用 Rust 重新实现的；界面以原版为准逐像素对照复刻。谨以此项目向 AirPlay3 的原作者致敬。

## 截图

| 播放中：大封面 + 歌曲信息 | 约 8 秒后：小封面 + 滚动歌词 |
| :---: | :---: |
| ![播放中](docs/images/screenshot-main.png) | ![歌词](docs/images/screenshot-lyrics.png) |

## 已实现

- 原版布局：播放条 + 歌词图片 + 均衡器 + 音乐列表，4 个吸附在一起的分层窗口，尺寸、字体、颜色与原版一致，可整组拖动
- 播放条：空闲时显示封面、AIRPLAY 与轮换标语；鼠标悬停淡入音量条、上一首 / 暂停 / 下一首、窗口按钮、面板开关、播放模式
- 歌词图片：大封面 + 歌曲信息浮层，有歌词时切换为小封面 + 同步滚动歌词（当前行橙色）
- 均衡器：前置放大 + 10 段，iTunes 风格预设（流行、摇滚、古典……）
- 音乐列表：分栏、斑马线、多选、拼音首字母搜索（如 `zjl` 搜到 周杰伦）、排序、删除不能播放的内容
- 原版风格右键菜单、托盘图标、全局热键、拖拽添加、窗口置顶
- 播放引擎：无缝衔接、CUE 分轨、WASAPI 共享 / 独占模式、续播上次位置
- 格式：MP3 / FLAC / OGG / WAV / AIFF / M4A(AAC、ALAC) / WMA / AC3 等；列表文件 m3u / m3u8 / pls / wpl / asx / xspf / ttpl / apl
- 本地歌词：LRC（GBK / UTF-8 自动识别、乱码修复、模糊匹配文件名），内嵌歌词与封面

## 尚未完成

- 「配置」窗口（目前点击会打开配置文件夹）
- 迷你条模式、桌面歌词（迷你歌词）
- 单实例运行、系统媒体控制（SMTC）
- 原版的在线电台、在线歌词等服务早已关闭，不再提供
- 不支持 APE / TAK / TTA / WV / MPC / DTS

## 使用

到 [Releases](../../releases) 下载 `AirPlay3.exe`，直接运行即可，无需安装。

- 与原版一样是绿色版：配置、曲库和歌词保存在程序旁边的 `AIRPLAY_CONFIG` 文件夹；该位置不可写时改用 `%APPDATA%\AirPlay3`
- 命令行：`AirPlay3.exe 文件或文件夹...` 打开并播放；`--tray` 启动时最小化到托盘
- 右键播放条打开主菜单；把音乐文件或文件夹拖进窗口即可添加

## 从源码构建

需要 Rust（`x86_64-pc-windows-gnu` 工具链）和完整的 MinGW-w64（提供 dlltool / windres）。

```bash
cp .cargo/config.example.toml .cargo/config.toml   # 按本机修改 MinGW 路径
cargo build --release
```

## 许可

代码以 [MIT](LICENSE) 协议开源。
