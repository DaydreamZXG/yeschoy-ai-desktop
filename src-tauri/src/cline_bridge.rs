//! Cline 的本地转发：`127.0.0.1:15733/cline/v1`。
//!
//! Cline 的 `providers.json` 里密钥只能明文写。和 OpenCode 一样，文件里只写
//! **本地令牌**（`ycg-…`），站点的密钥留在系统钥匙串里，由这座桥转发时取；
//! 套餐厂商要的会话标记也在转发时补上。
//!
//! Cline 的「OpenAI Compatible」说的是 Chat Completions，和 OpenCode 一样原样
//! 透传，所以路由直接用 `opencode_bridge` 那一套，这里只定自己的地址。
use crate::local_bridge::LocalBridgeRuntime;

pub(crate) const PROXY_ADDRESS: &str = "127.0.0.1:15733";
pub(crate) const PROXY_PREFIX: &str = "/cline";
pub(crate) const PROXY_BASE: &str = "http://127.0.0.1:15733/cline/v1";

/// 全局唯一的一座 Cline 桥。接入、打开、退出都走它。
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
    }
}
