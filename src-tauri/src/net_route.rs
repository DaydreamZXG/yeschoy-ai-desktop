//! 发往中转站的出站线路。
//!
//! 不少用户要开代理才连得上站点，用着用着开关代理、换节点也很常见。这里让发往站点的请求：
//! - 跟着系统代理走（先看环境变量，再看系统设置），每 5 秒重读一次，中途开关代理不用重启；
//! - 某个站在当前线路上连不上时记下来，之后几分钟改走另一条（有代理时在代理和直连之间换）；
//! - 只在连接没建立时补发一次：这时请求一个字节都没发出去，站点不会计费。
//!   已经建立的连接、已经开始的回答从不重发。

use std::cell::Cell;
use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use base64::Engine;
use hyper_util::client::proxy as outbound;
use outbound::matcher::Matcher;
use reqwest::{ClientBuilder, Proxy, RequestBuilder, Response, Url};
use tower::{Layer, Service};

const SYSTEM_REFRESH: Duration = Duration::from_secs(5);
const SWITCH_HOLD: Duration = Duration::from_secs(300);

struct SystemProxy {
    read_at: Instant,
    matcher: Arc<Matcher>,
}

static SYSTEM: Mutex<Option<SystemProxy>> = Mutex::new(None);
/// 站点 → 改走另一条线路到什么时候。
static SWITCHED: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static SWITCHES: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// 这次建连选了哪条线路；reqwest 在连接器的 `call` 里同步调用 [`pick`]。
    static PICKED: Cell<Option<Picked>> = const { Cell::new(None) };
}

struct Picked {
    host: String,
    via_proxy: bool,
}

impl Picked {
    fn failed(self) {
        let mut switched = SWITCHED.lock().unwrap_or_else(|e| e.into_inner());
        if self.via_proxy {
            switched.insert(self.host, Instant::now() + SWITCH_HOLD);
        } else {
            switched.remove(&self.host);
        }
        SWITCHES.fetch_add(1, Ordering::SeqCst);
    }
}

pub(crate) trait StationRoute {
    /// 跟着系统代理走，连不上就换线路。`connect_timeout` 是每条线路各自的建连时限。
    fn station_route(self, connect_timeout: Duration) -> Self;
}

impl StationRoute for ClientBuilder {
    fn station_route(self, connect_timeout: Duration) -> Self {
        self.proxy(Proxy::custom(pick))
            .connector_layer(SwitchOnFailureLayer {
                attempt: connect_timeout,
            })
    }
}

pub(crate) trait SendWithFallback {
    /// 和 `send` 一样；只是连接没建立就失败、而线路刚换过时，换条线路再发一次。
    fn send_with_fallback(self) -> impl Future<Output = reqwest::Result<Response>> + Send;
}

impl SendWithFallback for RequestBuilder {
    fn send_with_fallback(self) -> impl Future<Output = reqwest::Result<Response>> + Send {
        send(self)
    }
}

async fn send(request: RequestBuilder) -> reqwest::Result<Response> {
    let again = request.try_clone();
    let switches = SWITCHES.load(Ordering::SeqCst);
    let error = match request.send().await {
        Ok(response) => return Ok(response),
        Err(error) => error,
    };
    match again {
        Some(again) if error.is_connect() && SWITCHES.load(Ordering::SeqCst) != switches => {
            again.send().await
        }
        _ => Err(error),
    }
}

/// 只能给整个请求指定一条线路的场合（应用更新）：要试的线路，先试当前该走的那条。`None` 是直连。
pub(crate) fn routes_for(url: &Url) -> Vec<Option<Url>> {
    let Some(host) = remote_host(url) else {
        return vec![None];
    };
    let Some(proxy) = system_proxy(url) else {
        return vec![None];
    };
    if switched(&host) {
        vec![None, Some(proxy)]
    } else {
        vec![Some(proxy), None]
    }
}

/// 记下 [`routes_for`] 给的某条线路连不上，之后几分钟先试另一条。
pub(crate) fn route_failed(url: &Url, via_proxy: bool) {
    if let Some(host) = remote_host(url) {
        Picked { host, via_proxy }.failed();
    }
}

fn remote_host(url: &Url) -> Option<String> {
    let host = url
        .host_str()?
        .trim_matches(['[', ']'])
        .to_ascii_lowercase();
    let loopback = host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    (!loopback).then_some(host)
}

fn pick(url: &Url) -> Option<Url> {
    let host = remote_host(url)?;
    let proxy = system_proxy(url)?;
    let via_proxy = !switched(&host);
    PICKED.with(|picked| picked.set(Some(Picked { host, via_proxy })));
    via_proxy.then_some(proxy)
}

fn switched(host: &str) -> bool {
    let mut switched = SWITCHED.lock().unwrap_or_else(|e| e.into_inner());
    match switched.get(host) {
        Some(until) if *until > Instant::now() => true,
        Some(_) => {
            switched.remove(host);
            false
        }
        None => false,
    }
}

fn system_proxy(url: &Url) -> Option<Url> {
    let uri: http::Uri = url.as_str().parse().ok()?;
    let matcher = {
        let mut slot = SYSTEM.lock().unwrap_or_else(|e| e.into_inner());
        match slot.as_ref() {
            Some(system) if system.read_at.elapsed() < SYSTEM_REFRESH => system.matcher.clone(),
            _ => {
                let matcher = Arc::new(read_system());
                *slot = Some(SystemProxy {
                    read_at: Instant::now(),
                    matcher: matcher.clone(),
                });
                matcher
            }
        }
    };
    let intercept = matcher.intercept(&uri)?;
    let mut proxy = Url::parse(&intercept.uri().to_string()).ok()?;
    let credentials = intercept
        .raw_auth()
        .map(|(user, password)| (user.to_owned(), password.to_owned()))
        .or_else(|| {
            let value = intercept
                .basic_auth()?
                .to_str()
                .ok()?
                .strip_prefix("Basic ")?;
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(value)
                .ok()?;
            let (user, password) = String::from_utf8(decoded)
                .ok()?
                .split_once(':')
                .map(|(user, password)| (user.to_owned(), password.to_owned()))?;
            Some((user, password))
        });
    if let Some((user, password)) = credentials {
        proxy.set_username(&user).ok()?;
        proxy.set_password(Some(&password)).ok()?;
    }
    Some(proxy)
}

fn read_system() -> Matcher {
    #[cfg(windows)]
    if let Some(matcher) = windows_matcher() {
        return matcher;
    }
    #[cfg(target_os = "macos")]
    if let Some(matcher) = mac_socks_matcher() {
        return matcher;
    }
    Matcher::from_system()
}

#[cfg(any(windows, target_os = "macos"))]
fn proxy_env_set() -> bool {
    [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ]
    .iter()
    .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
}

/// hyper-util 在 macOS 上只读「网页代理 / 安全网页代理」，不读「SOCKS 代理」；
/// 只开了 SOCKS 的情况由这里接手。
#[cfg(target_os = "macos")]
fn mac_socks_matcher() -> Option<Matcher> {
    use system_configuration::core_foundation::array::CFArray;
    use system_configuration::core_foundation::base::{CFType, CFTypeRef, TCFType};
    use system_configuration::core_foundation::number::CFNumber;
    use system_configuration::core_foundation::string::{CFString, CFStringRef};
    use system_configuration::dynamic_store::SCDynamicStoreBuilder;
    use system_configuration::sys::schema_definitions::{
        kSCPropNetProxiesExceptionsList, kSCPropNetProxiesHTTPEnable, kSCPropNetProxiesHTTPSEnable,
        kSCPropNetProxiesSOCKSEnable, kSCPropNetProxiesSOCKSPort, kSCPropNetProxiesSOCKSProxy,
    };

    if proxy_env_set() {
        return None;
    }
    let proxies = SCDynamicStoreBuilder::new("yeschoy")
        .build()?
        .get_proxies()?;
    let number = |key: CFStringRef| {
        proxies
            .find(key)
            .and_then(|value| value.downcast::<CFNumber>())
            .and_then(|value| value.to_i32())
    };
    let (http, https, socks) = unsafe {
        (
            number(kSCPropNetProxiesHTTPEnable) == Some(1),
            number(kSCPropNetProxiesHTTPSEnable) == Some(1),
            number(kSCPropNetProxiesSOCKSEnable) == Some(1),
        )
    };
    if http || https || !socks {
        return None;
    }
    let host = proxies
        .find(unsafe { kSCPropNetProxiesSOCKSProxy })
        .and_then(|value| value.downcast::<CFString>())?
        .to_string();
    if host.is_empty() {
        return None;
    }
    let port = number(unsafe { kSCPropNetProxiesSOCKSPort }).unwrap_or(1080);
    let no = proxies
        .find(unsafe { kSCPropNetProxiesExceptionsList })
        .and_then(|value| value.downcast::<CFArray>())
        .map(|list| {
            list.get_all_values()
                .into_iter()
                .filter_map(|item| {
                    unsafe { CFType::wrap_under_get_rule(item as CFTypeRef) }.downcast::<CFString>()
                })
                .map(|item| item.to_string().replace("*.", ""))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host
    };
    let socks = format!("socks5h://{host}:{port}");
    Some(
        Matcher::builder()
            .http(socks.clone())
            .https(socks)
            .no(no)
            .build(),
    )
}

/// hyper-util 把 `ProxyServer` 整串当成一个地址，读不懂 v2rayN 这类软件写的
/// `http=127.0.0.1:10809;https=127.0.0.1:10809` 分协议写法，这种写法由这里接手。
#[cfg(windows)]
fn windows_matcher() -> Option<Matcher> {
    if proxy_env_set() {
        return None;
    }
    let settings = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    if settings.get_value::<u32, _>("ProxyEnable").unwrap_or(0) == 0 {
        return None;
    }
    let (http, https) = split_proxy_server(&settings.get_value::<String, _>("ProxyServer").ok()?)?;
    let no = ["NO_PROXY", "no_proxy"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
        .or_else(|| {
            settings
                .get_value::<String, _>("ProxyOverride")
                .ok()
                .map(|value| no_proxy_list(&value))
        })
        .unwrap_or_default();
    Some(Matcher::builder().http(http).https(https).no(no).build())
}

/// 分协议的 `ProxyServer` → (http 请求用的代理, https 请求用的代理)；不是分协议写法时返回 `None`。
#[cfg_attr(not(windows), allow(dead_code))]
fn split_proxy_server(value: &str) -> Option<(String, String)> {
    if !value.contains('=') {
        return None;
    }
    let (mut http, mut https, mut socks) = (None, None, None);
    for part in value.split(';') {
        let Some((scheme, address)) = part.split_once('=') else {
            continue;
        };
        let address = address.trim();
        if address.is_empty() {
            continue;
        }
        let with_scheme = |scheme: &str| {
            if address.contains("://") {
                address.to_owned()
            } else {
                format!("{scheme}://{address}")
            }
        };
        match scheme.trim().to_ascii_lowercase().as_str() {
            "http" => http = Some(with_scheme("http")),
            "https" => https = Some(with_scheme("http")),
            "socks" => socks = Some(with_scheme("socks5h")),
            _ => {}
        }
    }
    let http = http.or_else(|| socks.clone()).unwrap_or_default();
    let https = https.or(socks).unwrap_or_default();
    Some((http, https))
}

#[cfg_attr(not(windows), allow(dead_code))]
fn no_proxy_list(value: &str) -> String {
    value
        .split(';')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(",")
        .replace("*.", "")
}

#[derive(Clone)]
struct SwitchOnFailureLayer {
    attempt: Duration,
}

impl<S> Layer<S> for SwitchOnFailureLayer {
    type Service = SwitchOnFailure<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SwitchOnFailure {
            inner,
            attempt: self.attempt,
        }
    }
}

#[derive(Clone)]
struct SwitchOnFailure<S> {
    inner: S,
    attempt: Duration,
}

impl<S, R> Service<R> for SwitchOnFailure<S>
where
    S: Service<R>,
    S::Future: Send + 'static,
    S::Response: Send + 'static,
    S::Error: From<io::Error> + Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: R) -> Self::Future {
        PICKED.with(|picked| picked.take());
        let connecting = self.inner.call(request);
        let picked = PICKED.with(|picked| picked.take());
        let attempt = self.attempt;
        Box::pin(async move {
            let result = match tokio::time::timeout(attempt, connecting).await {
                Ok(result) => result,
                Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "connect timed out").into()),
            };
            if result.is_err() {
                if let Some(picked) = picked {
                    picked.failed();
                }
            }
            result
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn use_proxy(proxy: &str) {
        *SYSTEM.lock().unwrap() = Some(SystemProxy {
            read_at: Instant::now() + Duration::from_secs(3600),
            matcher: Arc::new(Matcher::builder().http(proxy).https(proxy).build()),
        });
        SWITCHED.lock().unwrap().clear();
    }

    #[tokio::test]
    async fn update_routes_follow_the_system_proxy_and_flip_after_a_failure() {
        let _serial = SERIAL.lock().await;
        use_proxy("http://127.0.0.1:7890");
        let url: Url = "https://github.com/o/r/releases/latest/download/latest.json"
            .parse()
            .unwrap();
        let proxy: Url = "http://127.0.0.1:7890".parse().unwrap();
        assert_eq!(routes_for(&url), vec![Some(proxy.clone()), None]);
        route_failed(&url, true);
        assert_eq!(routes_for(&url), vec![None, Some(proxy.clone())]);
        route_failed(&url, false);
        assert_eq!(routes_for(&url), vec![Some(proxy), None]);
        assert_eq!(
            routes_for(&"http://127.0.0.1:1/x".parse().unwrap()),
            vec![None]
        );
    }

    async fn closed_port() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    }

    async fn station() -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buffer = [0u8; 2048];
                let _ = socket.read(&mut buffer).await;
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    )
                    .await;
            }
        });
        address
    }

    #[test]
    fn windows_per_protocol_proxy_server_is_split() {
        assert_eq!(split_proxy_server("127.0.0.1:7890"), None);
        assert_eq!(
            split_proxy_server("http=127.0.0.1:10809;https=127.0.0.1:10808"),
            Some((
                "http://127.0.0.1:10809".into(),
                "http://127.0.0.1:10808".into()
            ))
        );
        assert_eq!(
            split_proxy_server("socks=127.0.0.1:1080"),
            Some((
                "socks5h://127.0.0.1:1080".into(),
                "socks5h://127.0.0.1:1080".into()
            ))
        );
        assert_eq!(
            split_proxy_server("http=127.0.0.1:8080;ftp=127.0.0.1:21"),
            Some(("http://127.0.0.1:8080".into(), String::new()))
        );
        assert_eq!(
            no_proxy_list("*.corp.test; localhost;<local>"),
            "corp.test,localhost,<local>"
        );
    }

    #[test]
    fn a_socks_proxy_is_handed_to_reqwest_as_socks() {
        let _serial = SERIAL.blocking_lock();
        use_proxy("socks5://127.0.0.1:7891");
        let proxy = pick(&Url::parse("https://station.test/").unwrap()).unwrap();
        assert_eq!(proxy.scheme(), "socks5");
        reqwest::Client::builder()
            .station_route(Duration::from_secs(2))
            .build()
            .unwrap();
    }

    #[test]
    fn loopback_never_goes_through_the_proxy() {
        let _serial = SERIAL.blocking_lock();
        use_proxy("http://192.0.2.1:7890");
        for url in [
            "http://127.0.0.1:1/x",
            "http://localhost:2/",
            "http://[::1]:3/",
        ] {
            assert_eq!(pick(&Url::parse(url).unwrap()), None, "{url}");
        }
        assert!(pick(&Url::parse("https://station.test/").unwrap()).is_some());
    }

    #[tokio::test]
    async fn a_dead_proxy_falls_back_to_direct_and_stays_there() {
        let _serial = SERIAL.lock().await;
        use_proxy(&format!("http://127.0.0.1:{}", closed_port().await));
        let station = station().await;
        let client = reqwest::Client::builder()
            .resolve("station.test", station)
            .station_route(Duration::from_secs(2))
            .build()
            .unwrap();
        let url = format!("http://station.test:{}/v1/models", station.port());

        let response = client.get(&url).send_with_fallback().await.unwrap();
        assert_eq!(response.text().await.unwrap(), "ok");
        assert!(switched("station.test"));

        let response = client.get(&url).send().await.unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn a_blocked_direct_route_goes_back_to_the_proxy() {
        let _serial = SERIAL.lock().await;
        let station = station().await;
        // 代理就是这个假站点：它对 CONNECT 之外的明文请求直接答 200。
        use_proxy(&format!("http://{station}"));
        SWITCHED
            .lock()
            .unwrap()
            .insert("station.test".into(), Instant::now() + SWITCH_HOLD);
        let client = reqwest::Client::builder()
            .resolve(
                "station.test",
                format!("127.0.0.1:{}", closed_port().await)
                    .parse()
                    .unwrap(),
            )
            .station_route(Duration::from_secs(2))
            .build()
            .unwrap();

        let response = client
            .get("http://station.test/v1/models")
            .send_with_fallback()
            .await
            .unwrap();
        assert_eq!(response.text().await.unwrap(), "ok");
        assert!(!switched("station.test"));
    }

    #[tokio::test]
    async fn a_failed_connect_without_a_switch_is_not_sent_twice() {
        let _serial = SERIAL.lock().await;
        *SYSTEM.lock().unwrap() = Some(SystemProxy {
            read_at: Instant::now() + Duration::from_secs(3600),
            matcher: Arc::new(Matcher::builder().build()),
        });
        let port = closed_port().await;
        let client = reqwest::Client::builder()
            .station_route(Duration::from_secs(2))
            .build()
            .unwrap();
        let before = SWITCHES.load(Ordering::SeqCst);
        let error = client
            .get(format!("http://127.0.0.1:{port}/"))
            .send_with_fallback()
            .await
            .unwrap_err();
        assert!(error.is_connect());
        assert_eq!(SWITCHES.load(Ordering::SeqCst), before);
    }
}
