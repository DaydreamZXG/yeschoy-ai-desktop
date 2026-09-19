//! 让中转赢过 claude.ai 的登录态，并且能还回去。
//!
//! 用户在野菜里给 Claude Code 选了模型，那就是他的选择。但 Claude Code 同时
//! 看得到两份凭据：我们写进 `~/.claude/settings.json` 的 `apiKeyHelper`，和它
//! 自己存在钥匙串里的 claude.ai 登录态。两个都在时它用后者，于是中转从头到尾
//! 没被用到 —— 用户在野菜里看到「设置已完成」，在 Claude Code 里看到的却是
//!
//!     Both claude.ai and apiKeyHelper set · auth may not work as expected
//!     Your account is on hold and can't use Claude Code.
//!
//! 配置文件那一半早就处理了（`claude_code.rs` 会删掉 `ANTHROPIC_AUTH_TOKEN`
//! 和 `ANTHROPIC_API_KEY`，注释写明是「so Claude Code cannot bypass
//! apiKeyHelper precedence」），但钥匙串里那份不是文件，写配置碰不到它。
//!
//! 所以这里把它挪走：接入时移到野菜自己的 service 下，恢复原设置时搬回去。
//! 挪走而不是删掉 —— 删掉就还不回来了，而「随时可以恢复原设置」是这个产品
//! 承诺过的事，不能为了省一步在这上面开口子。
//!
//! 两个 macOS 的事实决定了这里的形状：
//!
//!   · 探测不需要授权。`SecItemCopyMatching` 只要属性不要数据时不弹窗，所以
//!     「有没有登录态」可以静默判断 —— 这很重要，否则没登录 claude.ai 的人
//!     也会平白收到一个系统弹窗。
//!   · 真的读它一定弹窗。条目归 Claude Code 所有，野菜去读，系统必然问一次
//!     「是否允许」。这个绕不过去，所以调用方有义务在此之前把话说清楚，
//!     否则用户面对的是一个没头没尾的系统对话框，多数人会点拒绝。
use keyring::v1::{Entry, Error as KeyringError};

/// Claude Code 自己的钥匙串条目。
const CLAUDE_SERVICE: &str = "Claude Code-credentials";
/// 挪走之后暂存在这里，`restore` 从这里搬回去。
const BACKUP_SERVICE: &str = "com.yeschoy.desktop.displaced-login.v1";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// 用户在系统弹窗上点了拒绝，或钥匙串被锁。可重试。
    NotPermitted,
    /// 钥匙串本身不可用。
    Unavailable,
}

/// 接管的结果。调用方据此决定要不要提示用户。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// 本来就没有 claude.ai 登录态，什么都没做。
    NothingToDo,
    /// 登录态已挪到备份位，Claude Code 现在只能走 apiKeyHelper。
    Displaced,
}

fn account() -> String {
    // Claude Code 用登录用户名做 account（实测条目是 acct="<user>"）。
    std::env::var("USER").unwrap_or_default()
}

fn entry(service: &str, account: &str) -> Result<Entry, Failure> {
    Entry::new(service, account).map_err(|_| Failure::Unavailable)
}

fn map_read(error: KeyringError) -> Failure {
    match error {
        // keyring 把「用户拒绝/钥匙串锁着」归到平台错误里。它和「条目不存在」
        // 必须分开：前者重试有意义，后者没有。
        KeyringError::NoEntry => Failure::Unavailable,
        _ => Failure::NotPermitted,
    }
}

/// 有没有 claude.ai 登录态。**不读密文，所以不弹窗。**
///
/// 只查属性不查数据，正是 `security find-generic-password` 不带 `-w` 时的行为。
pub(crate) fn present() -> bool {
    #[cfg(target_os = "macos")]
    {
        // 只要属性、不要数据，钥匙串不会要求授权。
        std::process::Command::new("/usr/bin/security")
            .args(["find-generic-password", "-s", CLAUDE_SERVICE])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// 把 claude.ai 登录态挪到备份位。**会弹一次系统授权框。**
///
/// 调用方必须先向用户解释，否则那个框没有上下文。
pub(crate) fn take_over() -> Result<Outcome, Failure> {
    let account = account();
    let source = entry(CLAUDE_SERVICE, &account)?;
    let secret = match source.get_password() {
        Ok(secret) => secret,
        Err(KeyringError::NoEntry) => return Ok(Outcome::NothingToDo),
        Err(error) => return Err(map_read(error)),
    };
    // 先备份再删。反过来的话，删成功而备份失败就把用户的登录态弄丢了。
    entry(BACKUP_SERVICE, &account)?
        .set_password(&secret)
        .map_err(|_| Failure::Unavailable)?;
    match source.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(Outcome::Displaced),
        Err(error) => Err(map_read(error)),
    }
}

/// 把 claude.ai 登录态搬回 Claude Code。没有备份就什么都不做。
pub(crate) fn restore() -> Result<(), Failure> {
    let account = account();
    let backup = entry(BACKUP_SERVICE, &account)?;
    let secret = match backup.get_password() {
        Ok(secret) => secret,
        // 没接管过，或者已经还回去了 —— 都不是错误。
        Err(KeyringError::NoEntry) => return Ok(()),
        Err(error) => return Err(map_read(error)),
    };
    entry(CLAUDE_SERVICE, &account)?
        .set_password(&secret)
        .map_err(|_| Failure::Unavailable)?;
    // 还回去之后才丢备份，顺序反了会在写失败时两边都没有。
    let _ = backup.delete_credential();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这两个名字是契约的一部分：`CLAUDE_SERVICE` 必须精确等于 Claude Code
    /// 自己用的 service，写错了就既接管不了、也还不回去，而且不会报错 ——
    /// 只会表现为「接入完成了但中转没被用上」，正是这次要修的症状本身。
    #[test]
    fn the_service_names_are_pinned() {
        assert_eq!(CLAUDE_SERVICE, "Claude Code-credentials");
        assert_ne!(CLAUDE_SERVICE, BACKUP_SERVICE);
        // 备份位必须在野菜自己的命名空间下，不能污染别人的条目。
        assert!(BACKUP_SERVICE.starts_with("com.yeschoy.desktop."));
    }

    /// 「条目不存在」和「不让我读」必须分开。混在一起的话，用户点了拒绝会被
    /// 当成「他本来就没登录 claude.ai」，于是接入照常宣布成功，而中转依然没
    /// 被用上 —— 一个不会报错的错误。
    #[test]
    fn a_refusal_is_not_the_same_as_an_absence() {
        assert_eq!(map_read(KeyringError::NoEntry), Failure::Unavailable);
        assert_eq!(
            map_read(KeyringError::PlatformFailure(Box::new(
                std::io::Error::other("denied")
            ))),
            Failure::NotPermitted
        );
    }
}
