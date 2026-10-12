//! 手动运行的联网测试：几种常见的网络电台（MP3 直播流、HLS + TS、HLS + ADTS、HE-AAC），
//! 各听几秒，检查有声音、格式合理、没有中断。`cargo test --release --test radio_live -- --ignored --nocapture`

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
    // 手动测试时把地址换成真实的电台
    let cases = [
        ("MP3 直播流", "https://a.example.com/live.mp3"),
        ("Icecast MP3", "https://b.example.com/stream.mp3"),
        ("MP3 + ICY 节目名", "https://c.example.com/icy-128-mp3"),
        ("HLS + TS", "https://d.example.com/ts/index.m3u8"),
        ("HLS + ADTS", "https://e.example.com/adts/index.m3u8"),
        ("HLS 主播放列表 + ID3/ADTS", "https://f.example.com/master/playlist.m3u8"),
        ("AAC 直播流", "https://g.example.com/aac"),
    ];
    let mut ok = 0;
    for (name, url) in cases {
        match listen(url, 6.0) {
            Ok((rate, ch, secs, rms, info)) => {
                // 新闻台有时正好是停顿：有声音（不是全 0）就算正常
                let good = secs >= 5.0 && rms > 0.0001 && (8000..=96000).contains(&rate);
                ok += good as usize;
                println!("{} {name}: {rate} Hz {ch} 声道 听了 {secs:.1}s RMS {rms:.3}  {info}", if good { "OK" } else { "XX" });
            }
            Err(e) => println!("XX {name}: {e}"),
        }
    }
    assert!(ok >= 5, "只有 {ok} 个电台正常");
}
