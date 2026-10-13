use reqwest::Client;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use yts_core::{Provider, UpstreamStrategy};

/// 按配置指纹缓存 Client. 每个请求 new 一个 Client 会重做 TLS 握手、重建连接池,
/// 延迟和内存抖动都比"复用一条连接"贵得多.
pub fn client_for(provider: &Provider) -> anyhow::Result<Client> {
    static CLIENTS: OnceLock<Mutex<HashMap<String, Client>>> = OnceLock::new();
    let cache = CLIENTS.get_or_init(Default::default);
    let key = fingerprint(provider);

    if let Some(client) = cache.lock().unwrap().get(&key) {
        return Ok(client.clone());
    }
    let client = build_client(provider)?;
    let mut guard = cache.lock().unwrap();
    if guard.len() > 32 {
        guard.clear(); // 改过配置的旧指纹别一直堆着
    }
    guard.insert(key, client.clone());
    Ok(client)
}

fn fingerprint(p: &Provider) -> String {
    let proxy = match p.upstream_strategy {
        UpstreamStrategy::Direct => "direct".to_string(),
        UpstreamStrategy::SystemProxy => format!(
            "sys:{}",
            std::env::var("HTTPS_PROXY")
                .or_else(|_| std::env::var("https_proxy"))
                .unwrap_or_default()
        ),
        UpstreamStrategy::Custom => {
            format!("custom:{}", p.custom_proxy_url.clone().unwrap_or_default())
        }
    };
    format!("{:?}|{}|{}", p.protocol, p.base_url, proxy)
}

fn build_client(provider: &Provider) -> anyhow::Result<Client> {
    let mut builder = Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .connect_timeout(std::time::Duration::from_secs(10))
        .pool_max_idle_per_host(4)
        .no_proxy(); // default: ignore system proxy

    match provider.upstream_strategy {
        UpstreamStrategy::Direct => {
            // reqwest rustls doesn't auto-honor IE proxy unless we add it; Direct means no proxy
        }
        UpstreamStrategy::SystemProxy => {
            if let Ok(proxy) =
                std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("https_proxy"))
            {
                builder = builder.proxy(reqwest::Proxy::https(proxy)?);
            } else if let Ok(proxy) =
                std::env::var("HTTP_PROXY").or_else(|_| std::env::var("http_proxy"))
            {
                builder = builder.proxy(reqwest::Proxy::http(proxy)?);
            }
        }
        UpstreamStrategy::Custom => {
            if let Some(url) = &provider.custom_proxy_url {
                builder = builder.proxy(reqwest::Proxy::all(url.clone())?);
            }
        }
    }

    Ok(builder.build()?)
}

pub fn upstream_url(provider: &Provider, path: &str, query: Option<&str>) -> String {
    let base = provider.base_url.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    let mut url = format!("{}/{}", base, path);
    if let Some(q) = query {
        if !q.is_empty() {
            url.push('?');
            url.push_str(q);
        }
    }
    url
}
