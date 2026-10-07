import { invoke } from "@tauri-apps/api/core";

/**
 * 公告读取。
 *
 * 服务端契约（待实现，完整说明见 `docs/announcements-contract-zh.md`）：
 * bootstrap 增加可选字段 `announcements_path`，形如 `/api/desktop/v2/notices`；
 * 该路径带会话令牌 GET，返回公告列表。每条公告可以带一个按钮
 * （去充值 / 打开我们自己网站上的活动页）、一个截止时间，
 * 以及是否在首页顶部显示横幅。按钮去哪儿只有原生侧知道，界面只说「点了第几条」。
 *
 * 服务端还没宣告这个字段时 `available` 为 false，侧边栏不显示入口。
 * 客户端和服务端谁先发布都不会出错 —— 这正是 bootstrap 契约从
 * `deny_unknown_fields` 改成宽松之后才成立的事。
 */
export type NoticeSeverity = "info" | "warning";

export type Notice = {
  id: string;
  title: string;
  body: string;
  severity: NoticeSeverity;
  publishedAtEpochMs: number;
  /** 0 表示不过期。原生侧已经滤掉过期的，这里只用来显示截止日期。 */
  expiresAtEpochMs: number;
  /** 未读时在首页顶部显示横幅。 */
  banner: boolean;
  /** 按钮文字；空串表示这条公告没有按钮。 */
  actionLabel: string;
};

export type AnnouncementsProjection = {
  available: boolean;
  notices: Notice[];
};

const UNAVAILABLE: AnnouncementsProjection = { available: false, notices: [] };

function object(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** 公告是服务端自由文本，渲染前当不可信数据处理（后端已过滤一遍，这里是第二道）。 */
function epochMs(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0
    ? value
    : 0;
}

function decodeNotice(value: unknown): Notice | null {
  if (!object(value)) return null;
  const title = typeof value.title === "string" ? value.title : "";
  if (title.length === 0) return null;
  return {
    id: typeof value.id === "string" ? value.id : title,
    title,
    body: typeof value.body === "string" ? value.body : "",
    severity: value.severity === "warning" ? "warning" : "info",
    publishedAtEpochMs: epochMs(value.publishedAtEpochMs),
    expiresAtEpochMs: epochMs(value.expiresAtEpochMs),
    banner: value.banner === true,
    actionLabel:
      typeof value.actionLabel === "string"
        ? value.actionLabel.slice(0, 16)
        : "",
  };
}

export async function readAnnouncements(
  lineId: string,
): Promise<AnnouncementsProjection> {
  // 公告读不到只是没有公告。它绝不能影响登录或接入，所以这里吞掉所有错误。
  try {
    const result = await invoke("account_announcements_read_v2", {
      request: { requestId: `announcements-${Date.now()}`, lineId },
    });
    if (!object(result) || result.available !== true) return UNAVAILABLE;
    const notices = Array.isArray(result.notices)
      ? result.notices.map(decodeNotice).filter((n): n is Notice => n !== null)
      : [];
    return { available: true, notices };
  } catch {
    return UNAVAILABLE;
  }
}

/** 点公告上的按钮。去哪儿由原生侧按上次读到的公告决定。 */
export async function followAnnouncement(
  lineId: string,
  noticeId: string,
): Promise<boolean> {
  try {
    await invoke("account_announcement_action_v2", {
      request: {
        requestId: `announcement-action-${Date.now()}`,
        lineId,
        noticeId,
      },
    });
    return true;
  } catch {
    return false;
  }
}

/**
 * 读过哪些公告只是这台电脑上的一点便利（换台电脑再看一遍无妨），
 * 所以放 localStorage；读写失败就当全部未读，绝不影响页面。
 */
const SEEN_KEY = "yeschoy.announcements.seen.v1";
const SEEN_LIMIT = 200;

export function readSeenAnnouncements(): Set<string> {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(SEEN_KEY) ?? "[]");
    return new Set(
      Array.isArray(parsed)
        ? parsed.filter((id): id is string => typeof id === "string")
        : [],
    );
  } catch {
    return new Set();
  }
}

export function rememberSeenAnnouncements(
  seen: Set<string>,
  ids: string[],
): Set<string> {
  const next = new Set(seen);
  for (const id of ids) {
    // 重新插入，让最近看过的排在后面，截断时先丢最旧的。
    next.delete(id);
    next.add(id);
  }
  const trimmed = new Set([...next].slice(-SEEN_LIMIT));
  try {
    localStorage.setItem(SEEN_KEY, JSON.stringify([...trimmed]));
  } catch {
    // 存不下只是下次还显示未读。
  }
  return trimmed;
}
