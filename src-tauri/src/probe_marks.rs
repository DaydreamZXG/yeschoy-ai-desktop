//! 接入时的探测会真的在站点记一小笔用量（见 `tool_activation::probe_body`）。
//!
//! 站点的用量明细不会说哪一笔是探测，这里自己记下「哪个应用、哪些型号、
//! 什么时候探的」，账本读回来时把对得上的那几笔标成接入检查。只看应用、
//! 型号、时间和 token 数，不看站点背后走的是哪条线路，换线路也对得上。
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::account_finance::UsageRecord;

const FILE_NAME: &str = "probe-marks.json";
const MAX_MARKS: usize = 64;
/// 账本只看 30 天，多留一天够用。
const KEEP_MS: u64 = 31 * 86_400_000;
/// 站点的时间只记到秒，两边的钟也可能差一点。
const SLACK_MS: u64 = 5_000;
/// 探测只发一句 `hi`、最多回 16 个 token。比这多的是人自己发的，
/// 宁可漏标，也不把真用的那一笔说成检查。
const MAX_PROMPT_TOKENS: u64 = 64;
const MAX_COMPLETION_TOKENS: u64 = 16;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Mark {
    tool_id: String,
    models: Vec<String>,
    from_ms: u64,
    to_ms: u64,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkFile {
    schema_version: u8,
    marks: Vec<Mark>,
}

struct State {
    path: Option<PathBuf>,
    marks: Vec<Mark>,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(State {
            path: None,
            marks: Vec::new(),
        })
    })
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// 启动时载入。文件坏了就当没有：最多是少标几笔，不影响别的。
pub(crate) fn initialize(data_dir: Option<PathBuf>) {
    let Ok(mut guard) = state().lock() else {
        return;
    };
    let path = data_dir.map(|dir| dir.join(FILE_NAME));
    if let Some(text) = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
        if let Ok(file) = serde_json::from_str::<MarkFile>(&text) {
            if file.schema_version == 1 {
                guard.marks = file.marks;
            }
        }
    }
    guard.path = path;
}

/// 记下一次接入探了哪些型号。`from_ms`..`to_ms` 是这一批探测的起止。
pub(crate) fn record(tool_id: &str, models: Vec<String>, from_ms: u64, to_ms: u64) {
    if models.is_empty() {
        return;
    }
    let Ok(mut guard) = state().lock() else {
        return;
    };
    guard.marks.push(Mark {
        tool_id: tool_id.to_owned(),
        models,
        from_ms,
        to_ms,
    });
    prune(&mut guard.marks, to_ms);
    persist(&guard);
}

/// 把对得上探测的几笔标出来。
pub(crate) fn label(records: &mut [UsageRecord]) {
    let Ok(guard) = state().lock() else {
        return;
    };
    label_with(&guard.marks, records);
}

fn label_with(marks: &[Mark], records: &mut [UsageRecord]) {
    for record in records {
        record.probe = marks.iter().any(|mark| matches(mark, record));
    }
}

fn matches(mark: &Mark, record: &UsageRecord) -> bool {
    !record.tool_id.is_empty()
        && record.tool_id == mark.tool_id
        && record.prompt_tokens <= MAX_PROMPT_TOKENS
        && record.completion_tokens <= MAX_COMPLETION_TOKENS
        && record.observed_at_epoch_ms + SLACK_MS >= mark.from_ms
        && record.observed_at_epoch_ms <= mark.to_ms + SLACK_MS
        && mark
            .models
            .iter()
            .any(|model| model.eq_ignore_ascii_case(&record.model_id))
}

fn prune(marks: &mut Vec<Mark>, now: u64) {
    marks.retain(|mark| mark.to_ms + KEEP_MS >= now);
    let extra = marks.len().saturating_sub(MAX_MARKS);
    marks.drain(..extra);
}

fn persist(guard: &State) {
    let Some(path) = &guard.path else { return };
    let file = MarkFile {
        schema_version: 1,
        marks: guard.marks.clone(),
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(&file) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark() -> Mark {
        Mark {
            tool_id: "codex_desktop".into(),
            models: vec!["GLM-5.3-Flash".into()],
            from_ms: 1_000_000,
            to_ms: 1_020_000,
        }
    }

    fn record(at: u64, prompt: u64, completion: u64) -> UsageRecord {
        UsageRecord {
            tool_id: "codex_desktop".into(),
            model_id: "glm-5.3-flash".into(),
            observed_at_epoch_ms: at,
            prompt_tokens: prompt,
            completion_tokens: completion,
            cache_tokens: 0,
            amount: "0.0001".into(),
            probe: false,
        }
    }

    #[test]
    fn marks_only_the_small_calls_inside_the_probe_window() {
        let mut records = vec![
            // 站点只记到秒：比开始早一点也算。
            record(999_000, 8, 16),
            record(1_010_000, 8, 5),
            // 同一时间人自己发的：带着上下文，token 多。
            record(1_010_000, 4_000, 300),
            // 探测结束很久以后的「hi」。
            record(1_100_000, 8, 5),
        ];
        label_with(&[mark()], &mut records);
        let probes: Vec<bool> = records.iter().map(|r| r.probe).collect();
        assert_eq!(probes, [true, true, false, false]);
    }

    #[test]
    fn other_apps_and_models_are_not_marked() {
        let mut other_app = record(1_010_000, 8, 5);
        other_app.tool_id = "claude_code".into();
        let mut other_model = record(1_010_000, 8, 5);
        other_model.model_id = "kimi-k3".into();
        let mut unknown_app = record(1_010_000, 8, 5);
        unknown_app.tool_id.clear();
        let mut records = vec![other_app, other_model, unknown_app];
        label_with(&[mark()], &mut records);
        assert!(records.iter().all(|r| !r.probe));
    }

    #[test]
    fn old_marks_age_out_and_the_list_stays_bounded() {
        let mut marks: Vec<Mark> = (0..100)
            .map(|i| Mark {
                from_ms: KEEP_MS + i,
                to_ms: KEEP_MS + i,
                ..mark()
            })
            .collect();
        marks.insert(0, mark());
        prune(&mut marks, 2 * KEEP_MS);
        assert_eq!(marks.len(), MAX_MARKS);
        assert!(marks.iter().all(|m| m.to_ms >= KEEP_MS));
    }
}
