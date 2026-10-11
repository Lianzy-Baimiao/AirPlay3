//! 手动运行的联网测试：cargo test --test lrclib_live -- --ignored --nocapture
#[test]
#[ignore]
fn search_live() {
    let net = airplay3::config::NetworkSettings::default();
    let t = std::time::Instant::now();
    let r = airplay3::lyrics::lrclib::search(&net, "晴天", "周杰伦");
    println!("elapsed {:?} -> {:?}", t.elapsed(), r.as_ref().map(|v| v.len()).map_err(|e| e.clone()));
    assert!(r.is_ok());
}
