use reqwest::Client;
use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use clowder_core::{Provider, UpstreamStrategy};

/// 按配置指纹缓存 Client.
/// 解决“开梯子已有代理端口 + Clowder 统计又占一个端口”的二级代理串联问题:
/// 链路: Agent ──(直连 127.0.0.1:8787, NO_PROXY 绕过梯子)──> Clowder (:8787 统计 Token) ──(上行挂载梯子端口 :7890/:10808)──> 海外 API
pub fn client_for(provider: &Provider) -> anyhow::Result<Client> {
    static CLIENTS: OnceLock<Mutex<HashMap<String, Client>>> = OnceLock::new();
    let cache = CLIENTS.get_or_init(Default::default);
    let resolved_proxy = resolve_upstream_proxy(provider);
    let key = format!(
        "{:?}|{}|{}",
        provider.protocol,
        provider.base_url,
        resolved_proxy.as_deref().unwrap_or("direct")
    );

    if let Some(client) = cache.lock().unwrap().get(&key) {
        return Ok(client.clone());
    }
    let client = build_client(resolved_proxy.as_deref())?;
    let mut guard = cache.lock().unwrap();
    if guard.len() > 32 {
        guard.clear();
    }
    guard.insert(key, client.clone());
    Ok(client)
}

/// 解析上行出口实际使用的梯子代理地址:
/// - Direct: 不经过梯子，直接连上游 (适合国内 DeepSeek / Kimi / GLM / 硅基流动，开着梯子也不绕海外)
/// - SystemProxy:
///   1. 优先读取 Windows 注册表系统代理 (HKCU\...\Internet Settings 的 ProxyEnable + ProxyServer，Clash/v2rayN 开“系统代理”时写在这里)
///   2. 其次读取环境变量 HTTPS_PROXY / HTTP_PROXY / ALL_PROXY
///   3. 若都没读到，自动探测本机常用梯子端口 (7890 Clash / 7897 Clash Verge / 10809 & 10808 v2rayN / 2080 NekoBox)
/// - Custom: 使用用户手动指定的梯子端口 (如 http://127.0.0.1:7890 或 socks5://127.0.0.1:10808)
pub fn resolve_upstream_proxy(provider: &Provider) -> Option<String> {
    match provider.upstream_strategy {
        UpstreamStrategy::Direct => None,
        UpstreamStrategy::Custom => provider
            .custom_proxy_url
            .as_ref()
            .map(|s| normalize_proxy_url(s))
            .filter(|s| !s.is_empty()),
        UpstreamStrategy::SystemProxy => {
            if let Some(reg_proxy) = read_windows_registry_proxy() {
                return Some(reg_proxy);
            }
            if let Ok(env_proxy) = std::env::var("HTTPS_PROXY")
                .or_else(|_| std::env::var("https_proxy"))
                .or_else(|_| std::env::var("ALL_PROXY"))
                .or_else(|_| std::env::var("all_proxy"))
                .or_else(|_| std::env::var("HTTP_PROXY"))
                .or_else(|_| std::env::var("http_proxy"))
            {
                let norm = normalize_proxy_url(&env_proxy);
                if !norm.is_empty() {
                    return Some(norm);
                }
            }
            probe_common_local_proxy_ports()
        }
    }
}

fn normalize_proxy_url(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return String::new();
    }
    // Windows 注册表 ProxyServer 可能是 "127.0.0.1:7890" 或 "http=127.0.0.1:7890;https=127.0.0.1:7890"
    let extracted = if s.contains('=') {
        s.split(';')
            .find_map(|part| {
                let (k, v) = part.split_once('=')?;
                if k.eq_ignore_ascii_case("https") || k.eq_ignore_ascii_case("http") {
                    Some(v.trim())
                } else {
                    None
                }
            })
            .unwrap_or(s)
    } else {
        s
    };
    if extracted.starts_with("http://")
        || extracted.starts_with("https://")
        || extracted.starts_with("socks5://")
        || extracted.starts_with("socks5h://")
    {
        extracted.to_string()
    } else {
        format!("http://{}", extracted)
    }
}

#[cfg(windows)]
fn read_windows_registry_proxy() -> Option<String> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
        REG_SZ,
    };

    const SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings";
    let subkey_w: Vec<u16> = SUBKEY.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut hkey = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey_w.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return None;
        }

        // 1. 检查 ProxyEnable == 1
        let enable_name: Vec<u16> = "ProxyEnable"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut enabled: u32 = 0;
        let mut data_len: u32 = std::mem::size_of::<u32>() as u32;
        let mut reg_type: u32 = 0;
        let status = RegQueryValueExW(
            hkey,
            enable_name.as_ptr(),
            std::ptr::null_mut(),
            &mut reg_type,
            &mut enabled as *mut u32 as *mut u8,
            &mut data_len,
        );
        if status != 0 || reg_type != REG_DWORD || enabled == 0 {
            RegCloseKey(hkey);
            return None;
        }

        // 2. 读取 ProxyServer 字符串
        let server_name: Vec<u16> = "ProxyServer"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut buf = vec![0u16; 256];
        let mut buf_bytes: u32 = (buf.len() * 2) as u32;
        let status2 = RegQueryValueExW(
            hkey,
            server_name.as_ptr(),
            std::ptr::null_mut(),
            &mut reg_type,
            buf.as_mut_ptr() as *mut u8,
            &mut buf_bytes,
        );
        RegCloseKey(hkey);

        if status2 != 0 || reg_type != REG_SZ {
            return None;
        }
        let char_len = (buf_bytes as usize / 2).saturating_sub(1);
        let raw = String::from_utf16_lossy(&buf[..char_len]);
        let norm = normalize_proxy_url(&raw);
        if norm.is_empty() {
            None
        } else {
            Some(norm)
        }
    }
}

#[cfg(not(windows))]
fn read_windows_registry_proxy() -> Option<String> {
    None
}

/// 当用户开了 Clash / Clash Verge / v2rayN / NekoBox 但未勾选“系统代理”时，快速探测本机常见 HTTP 代理端口
fn probe_common_local_proxy_ports() -> Option<String> {
    const COMMON_PORTS: [u16; 5] = [
        7890,  // Clash / Mihomo 默认混合端口
        7897,  // Clash Verge Rev 默认端口
        10809, // v2rayN 默认 HTTP 端口
        10808, // v2rayN 默认端口
        2080,  // NekoBox 默认端口
    ];
    for port in COMMON_PORTS {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        if TcpStream::connect_timeout(&addr, Duration::from_millis(15)).is_ok() {
            return Some(format!("http://127.0.0.1:{}", port));
        }
    }
    None
}

fn build_client(resolved_proxy: Option<&str>) -> anyhow::Result<Client> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(4)
        .no_proxy(); // 先清空默认环境策略，再按供应商自己的上行策略精确挂载梯子

    if let Some(proxy_url) = resolved_proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url)?);
    }

    Ok(builder.build()?)
}

pub fn upstream_url(provider: &Provider, path: &str, query: Option<&str>) -> String {
    let base = provider.base_url.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    let mut url = if path.is_empty() {
        base.to_string()
    } else {
        format!("{}/{}", base, path)
    };
    if let Some(q) = query {
        if !q.is_empty() {
            url.push('?');
            url.push_str(q);
        }
    }
    url
}
