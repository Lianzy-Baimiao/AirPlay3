//! 自带解码器与 ffmpeg 逐样本对比（FFmpeg FATE 测试样本）。样本和参考 PCM 不在仓库里：
//! 在一个目录里放 `fate/`（样本）和 `refs/`（ffmpeg 解出的参考 PCM + index.json），然后
//! `AP3_FATE=<目录> cargo test --release --test codec_fate -- --ignored --nocapture`
//! 无损格式要求逐位相同；有损格式（Musepack、DTS）允许很小的误差。

use std::path::{Path, PathBuf};

use airplay3::audio::codec::{Block, open, open_decoder};

fn root() -> Option<PathBuf> {
    std::env::var_os("AP3_FATE").map(PathBuf::from)
}

struct Report {
    frames: usize,
    ref_frames: usize,
    /// 文件头说的总帧数
    total: u64,
    /// 第一个对不上的样本（交错序号）
    mismatch: Option<usize>,
    max_diff: f64,
    secs: f64,
}

/// 参考 PCM：s32le（整数，左对齐）或 f32le
fn reference(root: &Path, name: &str) -> Result<(String, usize, u32, Vec<f64>), String> {
    let idx: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("refs/index.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let info = &idx[name];
    let kind = info["kind"].as_str().ok_or(format!("{name} 没有参考 PCM"))?.to_string();
    let raw = std::fs::read(root.join("refs").join(format!("{name}.raw"))).map_err(|e| e.to_string())?;
    let v = raw
        .chunks_exact(4)
        .map(|c| {
            let b = [c[0], c[1], c[2], c[3]];
            if kind == "s32le" { i32::from_le_bytes(b) as f64 } else { f32::from_le_bytes(b) as f64 }
        })
        .collect();
    Ok((kind, info["channels"].as_u64().unwrap() as usize, info["rate"].as_u64().unwrap() as u32, v))
}

fn compare(root: &Path, name: &str, tol: f64) -> Result<Report, String> {
    let (kind, ch, rate, want) = reference(root, name)?;
    let mut dec = open_decoder(&root.join("fate").join(name)).ok_or("不认识的格式")?.map_err(|e| e.to_string())?;
    let spec = dec.spec();
    let total = dec.total_frames().unwrap_or(0);
    if spec.channels as usize != ch || spec.rate != rate {
        return Err(format!("格式不对：{}ch {}Hz，参考 {ch}ch {rate}Hz", spec.channels, spec.rate));
    }
    let t = std::time::Instant::now();
    let mut got: Vec<f64> = Vec::new();
    let mut b = Block::default();
    loop {
        let n = dec.decode(&mut b).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if !b.float.is_empty() {
            got.extend(b.float[..n * ch].iter().map(|&s| s as f64));
        } else if kind == "s32le" {
            got.extend(b.int[..n * ch].iter().map(|&s| s.wrapping_shl(32 - b.bits) as f64));
        } else {
            let sc = 1.0 / (1u64 << (b.bits - 1)) as f64;
            got.extend(b.int[..n * ch].iter().map(|&s| s as f64 * sc));
        }
    }
    let secs = t.elapsed().as_secs_f64();
    let common = got.len().min(want.len());
    let mut max_diff = 0f64;
    let mut mismatch = None;
    for i in 0..common {
        let d = (got[i] - want[i]).abs();
        max_diff = max_diff.max(d);
        if d > tol && mismatch.is_none() {
            mismatch = Some(i);
        }
    }
    Ok(Report { frames: got.len() / ch, ref_frames: want.len() / ch, total, mismatch, max_diff, secs })
}

/// 跳到中间某处再读，和参考 PCM 同一位置对比（f32）
fn check_seek(root: &Path, name: &str, tol: f64) -> Result<(), String> {
    let (kind, ch, _, want) = reference(root, name)?;
    let mut src = open(&root.join("fate").join(name)).ok_or("不认识的格式")?.map_err(|e| e.to_string())?;
    let frames = want.len() / ch;
    let at = frames / 2 + 1234;
    src.seek(at as u64).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let n = src.read(&mut buf).map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("跳转后读不到数据".into());
    }
    let scale = if kind == "s32le" { 1.0 / 2f64.powi(31) } else { 1.0 };
    for (k, &s) in buf.iter().take(4096 * ch).enumerate() {
        let w = want[at * ch + k] * scale;
        if (s as f64 - w).abs() > tol.max(1e-6) {
            return Err(format!("跳到 {at} 后第 {k} 个样本：{s} ≠ {w}"));
        }
    }
    Ok(())
}

fn run(exts: &[&str], tol: f64) {
    run_slack(exts, tol, 0);
}

/// 跳转后与参考解码器（同样跳转）的输出对比：参考 PCM 是 refs/<名字>.seek.raw，跳到 index.json 的 seek_at
fn check_seek_ref(root: &Path, name: &str, tol: f64) -> Result<(), String> {
    let idx: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("refs/index.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let at = idx[name]["seek_at"].as_u64().ok_or("没有 seek_at")?;
    let ch = idx[name]["channels"].as_u64().unwrap() as usize;
    let raw = std::fs::read(root.join("refs").join(format!("{name}.seek.raw"))).map_err(|e| e.to_string())?;
    let want: Vec<f32> = raw.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let mut src = open(&root.join("fate").join(name)).ok_or("不认识的格式")?.map_err(|e| e.to_string())?;
    src.seek(at).map_err(|e| e.to_string())?;
    let (mut got, mut buf) = (Vec::new(), Vec::new());
    while src.read(&mut buf).map_err(|e| e.to_string())? > 0 {
        got.extend_from_slice(&buf);
    }
    if got.len() != want.len() {
        return Err(format!("跳到 {at} 后解出 {} 帧，参考 {} 帧", got.len() / ch, want.len() / ch));
    }
    match got.iter().zip(&want).position(|(a, b)| (a - b).abs() as f64 > tol) {
        Some(i) => Err(format!("跳到 {at} 后第 {i} 个样本：{} ≠ {}", got[i], want[i])),
        None => Ok(()),
    }
}

/// `slack`：截断的文件（参考比文件头说的短）允许少解这么多帧。ffmpeg 对最后不完整的一帧会把读过头的
/// 填充也当数据解出来，我们只输出能确定的部分
fn run_slack(exts: &[&str], tol: f64, slack: usize) {
    let Some(root) = root() else {
        eprintln!("没有设置 AP3_FATE，跳过");
        return;
    };
    let mut names: Vec<String> = std::fs::read_dir(root.join("fate"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| exts.iter().any(|x| n.ends_with(x)) && !n.starts_with("encrypted"))
        .collect();
    names.sort();
    if names.is_empty() {
        eprintln!("{} 下没有 {exts:?} 样本，跳过", root.display());
        return;
    }
    let mut bad = Vec::new();
    for n in &names {
        match compare(&root, n, tol) {
            Ok(r) => {
                let dur = r.frames as f64 / 44_100.0;
                let short_ok = r.total > r.ref_frames as u64 && r.frames + slack >= r.ref_frames;
                let ok = r.mismatch.is_none() && (r.frames >= r.ref_frames || short_ok);
                println!(
                    "{} {n:34} {} / 参考 {} 帧，最大误差 {:.3e}{}  {:.0}x 实时",
                    if ok { "OK " } else { "XX " },
                    r.frames,
                    r.ref_frames,
                    r.max_diff,
                    r.mismatch.map_or(String::new(), |i| format!("，第 {i} 个样本起不同")),
                    dur / r.secs.max(1e-9)
                );
                if !ok {
                    bad.push(n.clone());
                } else if r.ref_frames > 10_000 {
                    if let Err(e) = check_seek(&root, n, tol) {
                        println!("XX {n} 跳转：{e}");
                        bad.push(n.clone());
                    }
                }
            }
            Err(e) => {
                println!("XX {n:34} {e}");
                bad.push(n.clone());
            }
        }
    }
    assert!(bad.is_empty(), "{} 个样本不对：{bad:?}", bad.len());
}

/// 带标签的 TTA（ffmpeg 生成：APEv2 在末尾）：标签经 lofty 读出，时长等由自带解码器给出
#[test]
#[ignore]
fn tta_tags() {
    let Some(root) = root() else { return };
    let p = root.join("tagged.tta");
    if !p.exists() {
        eprintln!("没有 tagged.tta，跳过");
        return;
    }
    let t = airplay3::media::metadata::read_tags(&p).expect("读不到标签");
    println!("{} / {} / {}  {:.2}s {}Hz {}ch {}kbps", t.title, t.artist, t.album, t.duration, t.sample_rate, t.channels, t.bitrate);
    assert_eq!((t.title.as_str(), t.artist.as_str(), t.album.as_str()), ("夜曲测试", "测试艺人", "测试专辑"));
    // 源文件（截断的 WV）只有 9.5 秒，ffprobe 也是 9.500
    assert!((t.duration - 9.5).abs() < 0.01 && t.sample_rate == 44_100 && t.channels == 2 && t.bitrate > 100);
}

/// 解码（最多约 100 万个样本），中间跳转两次；返回解出的帧数，打不开时是错误信息
fn decode_some(p: &Path) -> Result<u64, String> {
    let mut dec = open_decoder(p).ok_or("不认识")?.map_err(|e| e.to_string())?;
    let ch = dec.spec().channels.max(1) as u64;
    let mut b = Block::default();
    let mut total = 0u64;
    for round in 0..3u64 {
        let mut got = 0u64;
        while got * ch < 1_000_000 {
            let n = dec.decode(&mut b).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            got += n as u64;
        }
        total += got;
        dec.seek(dec.total_frames().unwrap_or(0) / (round + 2)).map_err(|e| e.to_string())?;
    }
    Ok(total)
}

/// 损坏的文件（改随机字节、一段填满、截断、改文件头附近）：解码不能 panic 或卡住。
/// 用 debug 跑（`cargo test --test codec_fate corrupt -- --ignored`）能顺便查出整数溢出
#[test]
#[ignore]
fn corrupt() {
    let Some(root) = root() else { return };
    let exts = std::env::var("AP3_CORRUPT_EXT").unwrap_or_else(|_| "tta,wv".into());
    let dir = std::env::temp_dir().join("ap3_corrupt");
    std::fs::create_dir_all(&dir).unwrap();
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join("fate"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| exts.split(',').any(|x| p.extension().is_some_and(|e| e == x)))
        .collect();
    files.sort();
    for p in &files {
        let data = std::fs::read(p).unwrap();
        let ext = p.extension().unwrap().to_string_lossy().into_owned();
        for k in 0..8u64 {
            let mut d = data.clone();
            let at = |r: u64, len: usize| (r % len.max(1) as u64) as usize;
            match k % 4 {
                0 => {
                    for _ in 0..1 + rnd() % 64 {
                        let i = at(rnd(), d.len());
                        d[i] = rnd() as u8;
                    }
                }
                1 => {
                    let i = at(rnd(), d.len());
                    let n = (rnd() % 4096) as usize;
                    let v = rnd() as u8;
                    let end = (i + n).min(d.len());
                    d[i..end].fill(v);
                }
                2 => d.truncate(at(rnd(), d.len())),
                _ => {
                    for _ in 0..4 {
                        let i = at(rnd(), d.len().min(256));
                        d[i] ^= 1 << (rnd() % 8);
                    }
                }
            }
            let f = dir.join(format!("c{k}.{ext}"));
            std::fs::write(&f, &d).unwrap();
            if std::env::var("AP3_CORRUPT_KEEP").is_ok_and(|v| p.ends_with(&v)) {
                std::fs::copy(&f, dir.with_file_name(format!("keep_c{k}.{ext}"))).unwrap();
            }
            let t = std::time::Instant::now();
            let r = decode_some(&f);
            println!("{} #{k}: {r:?} {:.2}s", p.file_name().unwrap().to_string_lossy(), t.elapsed().as_secs_f64());
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// 带 APEv2 标签的 TAK（Takc -tt 生成；lofty 不认识 TAK）：标签经标签载体读出，时长等由自带解码器给出
#[test]
#[ignore]
fn tak_tags() {
    let Some(root) = root() else { return };
    let p = root.join("tagged.tak");
    if !p.exists() {
        eprintln!("没有 tagged.tak，跳过");
        return;
    }
    let t = airplay3::media::metadata::read_tags(&p).expect("读不到标签");
    println!("{} / {} / {}  {:.2}s {}Hz {}ch {}kbps", t.title, t.artist, t.album, t.duration, t.sample_rate, t.channels, t.bitrate);
    assert_eq!((t.title.as_str(), t.artist.as_str(), t.album.as_str()), ("Tak Title", "Test Artist", "Test Album"));
    assert!((t.duration - 9.5).abs() < 0.01 && t.sample_rate == 44_100 && t.channels == 2 && t.bitrate > 100);
}

/// 浮点 APE（"MACF"，lofty 认不出）：标签经标签载体读出，时长等由自带解码器给出。
/// 样本由 apegen.py 生成后用 SDK 的 TagFileSimple 加上 APEv2 标签（AP3_FATE 指向 apeextra）
#[test]
#[ignore]
fn ape_float_tags() {
    let Some(root) = root() else { return };
    let p = root.join("tagged_f32.ape");
    if !p.exists() {
        eprintln!("没有 tagged_f32.ape，跳过");
        return;
    }
    let t = airplay3::media::metadata::read_tags(&p).expect("读不到标签");
    println!("{} / {} / {}  {:.2}s {}Hz {}ch {}kbps", t.title, t.artist, t.album, t.duration, t.sample_rate, t.channels, t.bitrate);
    assert_eq!((t.title.as_str(), t.artist.as_str(), t.album.as_str()), ("Float Title", "Test Artist", "Test Album"));
    assert!((t.duration - 9.5).abs() < 0.01 && t.sample_rate == 44_100 && t.channels == 2 && t.bitrate > 100);
}

/// Musepack：与 libmpcdec（浮点版）逐样本对比，包括跳转以后。AP3_FATE 指向 mpcgen.py 生成的 mpcextra
/// （FFmpeg FATE 的参考是定点解码、没有去掉开头 481 个采样点的合成延迟，见 mpc_ffmpeg）
#[test]
#[ignore]
fn mpc() {
    let Some(root) = root() else { return };
    let mut names: Vec<String> = std::fs::read_dir(root.join("fate")).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".mpc")).collect();
    names.sort();
    // 参考必须是 libmpcdec 解出来的（mpcgen.py 生成，带 seek_at）；FFmpeg 的参考见 mpc_ffmpeg
    let idx = std::fs::read_to_string(root.join("refs/index.json")).unwrap_or_default();
    if names.is_empty() || !idx.contains("seek_at") {
        eprintln!("{} 下没有 mpcgen.py 生成的 Musepack 样本，跳过", root.display());
        return;
    }
    let mut bad = Vec::new();
    for n in &names {
        let r = compare(&root, n, 0.0);
        let ok = matches!(&r, Ok(r) if r.mismatch.is_none() && r.frames == r.ref_frames);
        match &r {
            Ok(r) => println!("{} {n:20} {} / 参考 {} 帧，最大误差 {:.3e}  {:.0}x 实时", if ok { "OK " } else { "XX " }, r.frames, r.ref_frames, r.max_diff, r.frames as f64 / 44_100.0 / r.secs.max(1e-9)),
            Err(e) => println!("XX  {n:20} {e}"),
        }
        let seek = check_seek_ref(&root, n, 0.0);
        if let Err(e) = &seek {
            println!("XX  {n} {e}");
        }
        if !ok || seek.is_err() {
            bad.push(n.clone());
        }
    }
    assert!(bad.is_empty(), "{bad:?}");
}

/// Musepack 与 FFmpeg 的参考对比（FFmpeg 是定点解码，没有去掉合成延迟：它的第 481 个采样点是我们的第 0 个）。
/// AP3_FATE 指向 FFmpeg FATE 的样本目录（不是 mpcextra）
#[test]
#[ignore]
fn mpc_ffmpeg() {
    let Some(root) = root() else { return };
    let idx = std::fs::read_to_string(root.join("refs/index.json")).unwrap_or_default();
    if !idx.contains("musepack") {
        eprintln!("refs 不是 FFmpeg 的参考，跳过");
        return;
    }
    for n in ["inside-mp7.mpc", "inside-mp8.mpc"] {
        let (_, ch, _, want) = reference(&root, n).unwrap();
        let mut dec = open_decoder(&root.join("fate").join(n)).unwrap().unwrap();
        let (mut got, mut b) = (Vec::new(), Block::default());
        while dec.decode(&mut b).unwrap() > 0 {
            got.extend(b.float.iter().map(|&x| x as f64));
        }
        let off = 481 * ch;
        let common = got.len().min(want.len() - off);
        let max = (0..common).map(|i| (got[i] - want[i + off]).abs()).fold(0.0, f64::max);
        println!("{n}: {} 帧，与 ffmpeg 最大误差 {max:.3e}", got.len() / ch);
        assert!(max < 2e-3, "{n} 与 ffmpeg 差太多：{max}");
    }
}

/// Monkey's Audio：3.80 ~ 3.99 各版本、各压缩级别、24 位、截断的文件
#[test]
#[ignore]
fn ape() {
    run_slack(&[".ape"], 0.0, 73728);
}

/// WavPack：8 ~ 32 位整数、32 位浮点、有损（混合模式）、多声道、假立体声、截断的文件
#[test]
#[ignore]
fn wavpack() {
    run(&[".wv"], 0.0);
}

/// TAK：各预设、帧长、8 / 16 / 24 位、单声道 ~ 6 声道、8 ~ 192 kHz、极短的文件（takgen.py 生成，
/// 参考 PCM 由官方 Takc 解出）
#[test]
#[ignore]
fn tak() {
    run(&[".tak"], 0.0);
}

/// TAK 跳转到各处（开头、帧边界前后、最后一帧、末尾之后）和截断的文件。AP3_FATE 指向 takextra
#[test]
#[ignore]
fn tak_seek() {
    let Some(root) = root().filter(|r| r.join("fate/fsl512.tak").exists()) else {
        eprintln!("没有 takgen.py 生成的样本，跳过");
        return;
    };
    for name in ["p2.tak", "fsl512.tak", "r96k.tak", "c51.tak", "n11030.tak", "m8000_fsl512.tak"] {
        let (_, ch, _, want) = reference(&root, name).unwrap();
        let frames = want.len() / ch;
        let mut src = open(&root.join("fate").join(name)).unwrap().unwrap();
        for at in [0, 1, 511, 512, 5511, 5512, 5513, frames / 3, frames - 5000.min(frames / 2), frames - 1, frames, frames + 100] {
            src.seek(at as u64).unwrap();
            let (mut got, mut buf) = (Vec::new(), Vec::new());
            while got.len() < 4096 * ch && src.read(&mut buf).unwrap() > 0 {
                got.extend_from_slice(&buf);
            }
            let want = &want[(at.min(frames) * ch)..((at + 4096).min(frames) * ch)];
            assert!(got.len() >= want.len(), "{name} 跳到 {at}：只读到 {} 帧", got.len() / ch);
            for (k, (&g, &w)) in got.iter().zip(want).enumerate() {
                assert_eq!(g as f64, w / 2f64.powi(31), "{name} 跳到 {at} 后第 {k} 个样本");
            }
        }
        println!("{name}：跳转 OK");
    }
    // 截断的文件：能解的帧逐位相同，最后不完整的一帧是静音
    let data = std::fs::read(root.join("fate/p2.tak")).unwrap();
    let (_, ch, _, want) = reference(&root, "p2.tak").unwrap();
    for cut in [data.len() / 3, data.len() * 2 / 3 + 7, data.len() - 100] {
        let f = std::env::temp_dir().join("ap3_tak_cut.tak");
        std::fs::write(&f, &data[..cut]).unwrap();
        let mut dec = open_decoder(&f).unwrap().unwrap();
        let (mut got, mut b) = (Vec::new(), Block::default());
        while dec.decode(&mut b).unwrap() > 0 {
            got.extend(b.int.iter().map(|&s| (s as f64) * 65536.0));
        }
        let _ = std::fs::remove_file(&f);
        let n = got.len() / ch;
        let good = got.iter().zip(&want).take_while(|(g, w)| g == w).count() / ch;
        let silent = got[good * ch..].iter().all(|&s| s == 0.0);
        println!("截断到 {cut} 字节：解出 {n} 帧，前 {good} 帧相同，之后{}静音", if silent { "全是" } else { "不全是" });
        assert!(silent && n - good <= 5512 && n <= want.len() / ch, "截断到 {cut}：{n} 帧，{good} 帧相同");
    }
}

#[test]
#[ignore]
fn tta() {
    run(&[".tta"], 0.0);
    if let Some(root) = root().filter(|r| r.join("fate/encrypted.tta").exists()) {
        let e = open_decoder(&root.join("fate/encrypted.tta")).unwrap().err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("加密"), "加密的 TTA 应该给出明确的提示：{e}");
    }
}
