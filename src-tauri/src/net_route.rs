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

fn pick(url: &Url) -> Option<Url> {
    let host = url
        .host_str()?
        .trim_matches(['[', ']'])
        .to_ascii_lowercase();
    if host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
    {
        return None;
    }
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
                let matcher = Arc::new(Matcher::from_system());
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
