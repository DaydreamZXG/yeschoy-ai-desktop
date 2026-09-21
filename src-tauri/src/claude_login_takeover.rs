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
//!
//! `take_over` / `restore` 动的是真钥匙串（而且会弹授权框），单元测试里跑
//! 不了；能测的是解析那一层（`parse_account`）。调用它们的时机由
//! `tool_activation` 负责，那边的注释说明了每个调用点为什么在那里。
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

/// 登录用户名。条目自己报不出 account 时才退回到它 —— Claude Code 实测就是
/// 用它做 account（acct="<user>"），所以正常情况下两者相同。
fn login_user() -> String {
    std::env::var("USER").unwrap_or_default()
}

/// `security find-generic-password -s <service>` 的属性输出；条目不存在时 None。
///
/// **不带 `-w`，只要属性不要数据，所以不弹窗。**
fn attributes(service: &str) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/bin/security")
            .args(["find-generic-password", "-s", service])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = service;
        None
    }
}

/// 从 `security` 的属性输出里取 `acct`。
///
/// 两种写法：可打印的值是 `"acct"<blob>="<user>"`；含非 ASCII 字节时是
/// `"acct"<blob>=0x<hex>  "<escaped>"`。`<NULL>` 表示条目没有 account。
fn parse_account(attributes: &str) -> Option<String> {
    attributes.lines().find_map(|line| {
        let value = line.trim_start().strip_prefix("\"acct\"<blob>=")?;
        if let Some(quoted) = value.strip_prefix('"') {
            return quoted.strip_suffix('"').map(str::to_owned);
        }
        let hex = value.strip_prefix("0x")?.split_whitespace().next()?;
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(hex.get(index..index + 2)?, 16).ok())
            .collect::<Option<Vec<u8>>>()?;
        String::from_utf8(bytes).ok()
    })
}

/// 定位一条条目：不在就是 `None`；在，就用它自己报的 account，报不出来才
/// 退回登录用户名。
///
/// `present()` 只按 service 找，而 `Entry::new` 要 service 和 account 两个都
/// 对。以前 `take_over` 直接拿 `$USER` 当 account：`$USER` 没设、或和条目上
/// 的不一致时，它就找不到那条明明存在的登录态，回一个 `NothingToDo` ——
/// 接入照常宣布成功，中转依然没被用上，正是这个模块要修的那种不报错的错误。
/// 所以 account 从条目自己的属性里读，探测和接管两边的答案才是同一个；
/// 「看得见却定位不到」是错误，不是「没有」。
fn locate(service: &str) -> Result<Option<String>, Failure> {
    let Some(text) = attributes(service) else {
        return Ok(None);
    };
    parse_account(&text)
        .filter(|account| !account.is_empty())
        .or_else(|| Some(login_user()).filter(|user| !user.is_empty()))
        .map(Some)
        .ok_or(Failure::Unavailable)
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
    attributes(CLAUDE_SERVICE).is_some()
}

/// 把 claude.ai 登录态挪到备份位。**会弹一次系统授权框。**
///
/// 调用方必须先向用户解释，否则那个框没有上下文。
pub(crate) fn take_over() -> Result<Outcome, Failure> {
    // 和 `present()` 走同一条路找条目，两边的答案才是同一个。
    let Some(account) = locate(CLAUDE_SERVICE)? else {
        return Ok(Outcome::NothingToDo);
    };
    let source = entry(CLAUDE_SERVICE, &account)?;
    // 属性已经说条目在了。这里再拿到 `NoEntry` 是两条路对不上，不是「没有」
    // —— `map_read` 把它归成 `Unavailable`，接入据此失败而不是假装成功。
    let secret = source.get_password().map_err(map_read)?;
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
///
/// **Claude Code 里现在已经有一条登录态时不动它。** 接管之后用户可能又在
/// Claude Code 里登录了一次 —— 那条是他刚做的选择，拿旧备份盖掉等于替他
/// 撤销了这次登录，而且旧的那份多半已经过期。这时备份只是被新登录取代了，
/// 丢掉它，不留一份将来会在他登出之后又冒出来的旧凭据。
///
/// 「有没有」只查属性不读数据（和 `present` 同一条路），所以不弹窗；真去读
/// 那条新登录态才会弹，而这里没有任何理由要读它。
pub(crate) fn restore() -> Result<(), Failure> {
    // 没接管过，或者已经还回去了 —— 都不是错误。
    let Some(account) = locate(BACKUP_SERVICE)? else {
        return Ok(());
    };
    let backup = entry(BACKUP_SERVICE, &account)?;
    let secret = backup.get_password().map_err(map_read)?;
    if present() {
        log::info!("claude_login_takeover stage=restore result=newer_login_kept backup=discarded");
        let _ = backup.delete_credential();
        return Ok(());
    }
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

    /// account 从条目自己的属性里读，而不是从 `$USER` 猜。
    ///
    /// 猜错的后果是 `take_over` 回 `NothingToDo`，接入宣布成功而登录态还在。
    /// `take_over` / `restore` 本身要动真钥匙串（而且会弹授权框），单元测试
    /// 里跑不了；能钉住的是解析这一层 —— `security` 的两种输出形状都得认。
    #[test]
    fn the_account_is_read_from_the_entry_not_guessed() {
        let printable = "keychain: \"/Users/x/Library/Keychains/login.keychain-db\"\n\
                         class: \"genp\"\n\
                         attributes:\n    \
                         \"acct\"<blob>=\"zxg\"\n    \
                         \"svce\"<blob>=\"Claude Code-credentials\"\n";
        assert_eq!(parse_account(printable).as_deref(), Some("zxg"));

        // 非 ASCII 用户名：security 改印十六进制，后面再跟一份转义过的副本。
        let hex = "    \"acct\"<blob>=0xE4BBA3E885BEE6B091  \"\\344\\273\\243...\"\n";
        assert_eq!(parse_account(hex).as_deref(), Some("代腾民"));

        // 没有 account 的条目：不能把 `<NULL>` 当成用户名。
        assert_eq!(parse_account("    \"acct\"<blob>=<NULL>\n"), None);
        assert_eq!(parse_account(""), None);
        // `svce` 那一行长得很像，绝不能误认。
        assert_eq!(
            parse_account("    \"svce\"<blob>=\"Claude Code-credentials\"\n"),
            None
        );
    }
}
