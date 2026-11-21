//! Hermes Agent 的本地转发：`127.0.0.1:15734/hermes/v1`。
//!
//! Hermes 的 `config.yaml` 里写的是**本地令牌**（`ycg-…`），站点的密钥留在
//! 系统钥匙串里，由这座桥转发时取；套餐厂商要的会话标记也在转发时补上。
//!
//! Hermes 接自定义地址时说的是 Chat Completions，和 OpenCode、Cline 一样原样
//! 透传，所以路由直接用 `opencode_bridge` 那一套，这里只定自己的地址。
//! 它启动时还会探几个别家的路径（`/api/show`、`/props` 之类，认 Ollama 和
//! llama.cpp 用的），桥一律回 404，Hermes 照常工作（0.0.0 @15806b32 实测）。
use crate::local_bridge::LocalBridgeRuntime;

pub(crate) const PROXY_ADDRESS: &str = "127.0.0.1:15734";
pub(crate) const PROXY_PREFIX: &str = "/hermes";
pub(crate) const PROXY_BASE: &str = "http://127.0.0.1:15734/hermes/v1";

/// 全局唯一的一座 Hermes 桥。接入、打开、退出都走它。
pub(crate) fn global() -> &'static LocalBridgeRuntime {
    static RUNTIME: std::sync::OnceLock<LocalBridgeRuntime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| {
        LocalBridgeRuntime::new(PROXY_ADDRESS, PROXY_PREFIX, crate::opencode_bridge::router)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_matches_address_and_prefix() {
        assert_eq!(
            PROXY_BASE,
            format!("http://{PROXY_ADDRESS}{PROXY_PREFIX}/v1")
        );
        assert_ne!(PROXY_ADDRESS, crate::opencode_bridge::PROXY_ADDRESS);
        assert_ne!(PROXY_ADDRESS, crate::cline_bridge::PROXY_ADDRESS);
    }
}
