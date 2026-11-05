//! 手动查询本项目的 GitHub 最新正式版；只读取版本信息，不下载或安装程序。

use std::cmp::Ordering;

use crate::config::NetworkSettings;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PROJECT_URL: &str = "https://github.com/Lianzy-Baimiao/AirPlay3";
pub const ISSUES_URL: &str = "https://github.com/Lianzy-Baimiao/AirPlay3/issues";
pub const RELEASES_URL: &str = "https://github.com/Lianzy-Baimiao/AirPlay3/releases/latest";
const API_URL: &str = "https://api.github.com/repos/Lianzy-Baimiao/AirPlay3/releases/latest";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    /// GitHub 版本相对当前版本的大小；本地开发版本可能比正式版更新。
    pub comparison: Ordering,
    pub url: String,
}

#[derive(serde::Deserialize)]
struct ReleaseInfo {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}

/// 本项目使用 v主版本.次版本.修订号；逐段按数字比较，不能用字符串排序。
fn version(tag: &str) -> Option<[u64; 3]> {
    if tag.len() > 32 { return None; }
    let mut parts = tag.strip_prefix('v').unwrap_or(tag).split('.');
    let mut v = [0; 3];
    for n in &mut v {
        let p = parts.next()?;
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) { return None; }
        *n = p.parse().ok()?;
    }
    parts.next().is_none().then_some(v)
}

fn release(info: ReleaseInfo, current: &str) -> Result<Release, String> {
    let invalid = || "GitHub 返回的正式版信息无效，请稍后重试".to_string();
    if info.draft || info.prerelease { return Err(invalid()); }
    let remote = version(&info.tag_name).ok_or_else(invalid)?;
    let local = version(current).ok_or_else(|| "无法识别当前程序的版本号".to_string())?;
    // 链接始终指向本仓库；只使用校验过的数字版本标签。
    let url = format!("{PROJECT_URL}/releases/tag/{}", info.tag_name);
    Ok(Release { tag: info.tag_name, comparison: remote.cmp(&local), url })
}

pub fn check(net: &NetworkSettings) -> Result<Release, String> {
    let agent = crate::lyrics::lrclib::agent(net).map_err(|_| "代理设置无效，请检查「网络连接」配置".to_string())?;
    check_with(&agent, API_URL, VERSION)
}

fn check_with(agent: &ureq::Agent, url: &str, current: &str) -> Result<Release, String> {
    let mut response = agent.get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call().map_err(|_| "连接 GitHub 失败，请检查网络或代理后重试".to_string())?;
    match response.status().as_u16() {
        200 => {}
        403 | 429 => return Err("GitHub 暂时限制了请求，请稍后重试".into()),
        404 => return Err("GitHub 上暂时没有可用的正式版".into()),
        500..=599 => return Err("GitHub 服务暂时不可用，请稍后重试".into()),
        status => return Err(format!("检查失败：GitHub 返回 HTTP {status}")),
    }
    let info = response.body_mut().with_config().limit(512 * 1024).read_json::<ReleaseInfo>()
        .map_err(|_| "GitHub 返回的版本信息无法读取，请重试".to_string())?;
    release(info, current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(tag: &str) -> ReleaseInfo {
        ReleaseInfo { tag_name: tag.into(), draft: false, prerelease: false }
    }

    #[test]
    fn compares_numeric_versions_and_rejects_non_releases() {
        assert_eq!(release(info("v3.10.0"), "3.9.9").unwrap().comparison, Ordering::Greater);
        assert_eq!(release(info("v3.27.10"), "3.27.9").unwrap().comparison, Ordering::Greater);
        assert_eq!(release(info("v3.27.7"), "3.27.7").unwrap().comparison, Ordering::Equal);
        assert_eq!(release(info("3.27.6"), "3.27.7").unwrap().comparison, Ordering::Less);
        assert_eq!(release(info("v4.0.0"), "3.99.99").unwrap().url, format!("{PROJECT_URL}/releases/tag/v4.0.0"));
        for tag in ["", "latest", "v3.2", "v3.2.1.0", "v3.2.1-beta", "v3.2.-1", "https://example.com", "v3.2.1/../../other"] {
            assert!(release(info(tag), VERSION).is_err(), "{tag}");
        }
        let mut draft = info("v4.0.0");
        draft.draft = true;
        assert!(release(draft, VERSION).is_err());
        let mut pre = info("v4.0.0");
        pre.prerelease = true;
        assert!(release(pre, VERSION).is_err());
    }

    /// 用本地 HTTP 响应走完整查询路径，不依赖 GitHub、系统代理或网络状态。
    #[test]
    fn http_results_errors_and_request_headers() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::time::Duration;
        for (status, body, expected) in [
            (200, r#"{"tag_name":"v3.28.0","draft":false,"prerelease":false}"#, "v3.28.0"),
            (200, "not json", "无法读取"),
            (200, r#"{"tag_name":"v9.0.0","draft":false,"prerelease":true}"#, "无效"),
            (404, "{}", "没有可用"),
            (403, "{}", "限制"),
            (429, "{}", "限制"),
            (503, "{}", "暂时不可用"),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/latest", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                String::from_utf8(request).unwrap().to_ascii_lowercase()
            });
            let agent: ureq::Agent = ureq::Agent::config_builder().proxy(None).http_status_as_error(false)
                .timeout_global(Some(Duration::from_secs(2))).build().into();
            let result = check_with(&agent, &url, "3.27.7");
            if expected.starts_with('v') {
                let result = result.unwrap();
                assert_eq!(result.tag, expected);
                assert_eq!(result.comparison, Ordering::Greater);
            } else {
                assert!(result.unwrap_err().contains(expected));
            }
            let request = server.join().unwrap();
            assert!(request.starts_with("get /latest http/1.1"));
            assert!(request.contains("accept: application/vnd.github+json"));
            assert!(request.contains("x-github-api-version: 2022-11-28"));
        }
    }

    #[test]
    #[ignore = "手动验证 GitHub 连接，需要联网"]
    fn github_live() {
        let result = check(&NetworkSettings::default()).unwrap();
        println!("current={VERSION}, latest={}, comparison={:?}, url={}", result.tag, result.comparison, result.url);
    }
}
