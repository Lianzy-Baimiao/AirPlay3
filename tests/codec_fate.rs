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
    Ok(Report { frames: got.len() / ch, ref_frames: want.len() / ch, mismatch, max_diff, secs })
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
    let mut bad = Vec::new();
    for n in &names {
        match compare(&root, n, tol) {
            Ok(r) => {
                let dur = r.frames as f64 / 44_100.0;
                let ok = r.mismatch.is_none() && r.frames >= r.ref_frames;
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
    assert!(!names.is_empty(), "没有 {exts:?} 样本");
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

#[test]
#[ignore]
fn tta() {
    run(&[".tta"], 0.0);
    if let Some(root) = root() {
        let e = open_decoder(&root.join("fate/encrypted.tta")).unwrap().err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("加密"), "加密的 TTA 应该给出明确的提示：{e}");
    }
}
