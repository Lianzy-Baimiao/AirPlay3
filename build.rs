//! 嵌入 Windows 资源：程序图标、清单（DPI 感知、通用控件 v6）、版本信息。
//! 使用 MinGW 的 windres（路径来自 .cargo/config.toml 的 AIRPLAY_MINGW_BIN，否则从 PATH 查找）。

use std::path::PathBuf;
use std::process::Command;

fn main() {
    for f in ["build.rs", "assets/app.rc", "assets/app.ico", "assets/app.manifest"] {
        println!("cargo:rerun-if-changed={f}");
    }
    println!("cargo:rerun-if-env-changed=AIRPLAY_MINGW_BIN");
    if !std::env::var("TARGET").unwrap_or_default().contains("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("app_res.o");
    let bin = std::env::var("AIRPLAY_MINGW_BIN").ok();
    let windres = bin.as_ref().map_or("windres".to_string(), |b| format!("{b}/windres.exe"));
    let mut cmd = Command::new(&windres);
    cmd.current_dir("assets").args(["app.rc", "-O", "coff", "-o"]).arg(&out);
    if let Some(b) = &bin {
        // windres 会调用 gcc 做预处理
        let path = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{b};{path}"));
    }
    match cmd.status() {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", out.display()),
        _ => println!("cargo:warning=windres 不可用，程序将不带图标与清单"),
    }
}
