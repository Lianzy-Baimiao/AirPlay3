//! 手动运行的联网测试：把自己的电台清单（M3U、PLS，或每行「名字,地址」）的路径放进 AIRPLAY3_RADIO_LIST，
//! 每个电台听几秒，检查有声音、格式合理、没有中断（MP3 直播流、HLS + TS / ADTS、HE-AAC 等）。仓库里没有电台地址。
//! `AIRPLAY3_RADIO_LIST=清单.txt cargo test --release --test radio_live -- --ignored --nocapture`

use airplay3::audio::source::open_source;

fn listen(url: &str, secs: f64) -> Result<(u32, u16, f64, f64, String), String> {
    let t0 = std::time::Instant::now();
    let mut s = open_source(std::path::Path::new(url)).map_err(|e| e.to_string())?;
    let opened = t0.elapsed().as_secs_f64();
    let mut buf = Vec::new();
    let (mut frames, mut sum, mut n, mut peak) = (0u64, 0f64, 0u64, 0f32);
    while (frames as f64) < secs * s.spec().rate as f64 && t0.elapsed().as_secs_f64() < secs * 3.0 + 15.0 {
        let k = s.read(&mut buf).map_err(|e| e.to_string())?;
        if k == 0 && !s.live() {
            break;
        }
        frames += k as u64;
        for &v in &buf {
            sum += (v as f64) * (v as f64);
            n += 1;
            peak = peak.max(v.abs());
        }
    }
    let spec = s.spec();
    let rms = if n > 0 { (sum / n as f64).sqrt() } else { 0.0 };
    let st = airplay3::audio::stream::status();
    Ok((spec.rate, spec.channels, frames as f64 / spec.rate as f64, rms, format!("峰值 {peak:.3}，{opened:.1}s 连上；{} 「{}」", st.info, st.title)))
}

#[test]
#[ignore]
fn stations() {
    let Some(path) = std::env::var_os("AIRPLAY3_RADIO_LIST") else {
        println!("没有设置 AIRPLAY3_RADIO_LIST（自己的电台清单文件），跳过");
        return;
    };
    let text = std::fs::read_to_string(&path).expect("读不了电台清单");
    let list = airplay3::radio::parse_list(&text);
    assert!(!list.is_empty(), "清单里没有电台");
    let mut ok = 0;
    for st in &list {
        match listen(&st.url, 6.0) {
            Ok((rate, ch, secs, rms, info)) => {
                // 新闻台有时正好是停顿：有声音（不是全 0）就算正常
                let good = secs >= 5.0 && rms > 0.0001 && (8000..=96000).contains(&rate);
                ok += good as usize;
                println!("{} {}: {rate} Hz {ch} 声道 听了 {secs:.1}s RMS {rms:.3}  {info}", if good { "OK" } else { "XX" }, st.name);
            }
            Err(e) => println!("XX {}: {e}", st.name),
        }
    }
    assert!(ok * 10 >= list.len() * 7, "只有 {ok} / {} 个电台正常", list.len());
}
