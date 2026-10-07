import { useState, type ReactNode } from "react";
import {
  ArrowUpRight,
  Megaphone,
  TriangleAlert,
  RefreshCw,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import type { Notice } from "./announcements";
import { useWorkbenchCopy } from "./copy";

// 日期跟着界面语言走。以前写死 `zh-CN`，英文界面上会冒出「2026年9月21日」。
function publishedAt(epochMs: number, locale: string): string {
  if (epochMs <= 0) return "";
  return new Date(epochMs).toLocaleDateString(locale, {
    year: "numeric",
    month: "long",
    day: "numeric",
  });
}

/**
 * 公告正文只认一种标记：`**加粗**`（后台运营习惯这么写）。其余一律当纯文本，
 * 不解析 HTML、链接或别的 Markdown。`**` 不成对时，落单的那个原样显示。
 */
export function noticeLine(line: string): ReactNode[] {
  const parts = line.split("**");
  if (parts.length % 2 === 0) {
    const tail = parts.pop() ?? "";
    parts[parts.length - 1] += `**${tail}`;
  }
  return parts.map((part, index) =>
    index % 2 === 1 ? <strong key={index}>{part}</strong> : part,
  );
}

function noticeParagraphs(body: string): string[] {
  return body.split("\n").filter((line) => line.trim().length > 0);
}

function NoticeAction({
  notice,
  onFollow,
}: {
  notice: Notice;
  onFollow: (notice: Notice) => void;
}) {
  if (!notice.actionLabel) return null;
  return (
    <button
      type="button"
      className="announcement-action"
      onClick={() => onFollow(notice)}
    >
      {notice.actionLabel}
      <ArrowUpRight aria-hidden="true" />
    </button>
  );
}

export function AnnouncementsView({
  notices,
  loading,
  failed,
  onRetry,
  unread,
  onFollow,
}: {
  notices: Notice[];
  loading: boolean;
  failed: boolean;
  onRetry: () => void;
  /** 进入页面时还没读过的公告，标「新」。 */
  unread: ReadonlySet<string>;
  onFollow: (notice: Notice) => void;
}) {
  const c = useWorkbenchCopy();
  const { i18n } = useTranslation();
  const locale = i18n.language;
  // 一进页面就会全部记为已读；「新」的标记按进来那一刻算，看的过程中不消失。
  const [unreadOnArrival] = useState(() => new Set(unread));
  return (
    <div className="workspace announcements-view" id="top">
      <div className="workbench-page-heading">
        <div>
          <h1>{c.announcementsTitle}</h1>
          <p>{c.announcementsIntro}</p>
        </div>
        <button type="button" className="subtle-button" onClick={onRetry}>
          <RefreshCw
            aria-hidden="true"
            className={loading ? "is-spinning" : undefined}
          />
          {c.announcementsRetry}
        </button>
      </div>

      {/* 定时刷新失败一次不该把正在看的公告清掉：提示放在上面，列表照留。 */}
      {failed && (
        <p className="workbench-notice" role="status">
          <TriangleAlert aria-hidden="true" />
          {c.announcementsFailed}
        </p>
      )}
      {notices.length === 0 ? (
        !loading &&
        !failed && (
          <p className="announcements-empty">
            <Megaphone aria-hidden="true" />
            {c.announcementsEmpty}
          </p>
        )
      ) : (
        <ol className="announcement-list">
          {notices.map((notice) => (
            <li
              key={notice.id}
              className="announcement-card"
              data-severity={notice.severity}
            >
              <div className="announcement-heading">
                <h2>
                  {(unreadOnArrival.has(notice.id) ||
                    unread.has(notice.id)) && (
                    <span className="announcement-new">
                      {c.announcementsUnread}
                    </span>
                  )}
                  {notice.title}
                </h2>
                {publishedAt(notice.publishedAtEpochMs, locale) && (
                  <time
                    dateTime={new Date(notice.publishedAtEpochMs).toISOString()}
                  >
                    {publishedAt(notice.publishedAtEpochMs, locale)}
                  </time>
                )}
              </div>
              {/* 服务端自由文本：按换行分段，只认 **加粗**，绝不当 HTML 解析。 */}
              {noticeParagraphs(notice.body).map((line, index) => (
                <p key={index}>{noticeLine(line)}</p>
              ))}
              {(notice.actionLabel || notice.expiresAtEpochMs > 0) && (
                <div className="announcement-footer">
                  <NoticeAction notice={notice} onFollow={onFollow} />
                  {notice.expiresAtEpochMs > 0 && (
                    <small>
                      {c.announcementEndsAt.replace(
                        "{{date}}",
                        publishedAt(notice.expiresAtEpochMs, locale),
                      )}
                    </small>
                  )}
                </div>
              )}
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}

/**
 * 首页顶部的公告横幅：只显示一条（未读里最靠前、服务端标了 banner 的），
 * 点按钮、点「查看全部」或关掉都算读过，之后不再出现。
 */
export function AnnouncementBanner({
  notice,
  onFollow,
  onOpenAll,
  onDismiss,
}: {
  notice: Notice;
  onFollow: (notice: Notice) => void;
  onOpenAll: () => void;
  onDismiss: (notice: Notice) => void;
}) {
  const c = useWorkbenchCopy();
  const summary = noticeParagraphs(notice.body)[0];
  return (
    <section
      className="announcement-banner"
      data-severity={notice.severity}
      aria-label={c.announcements}
      data-testid="announcement-banner"
    >
      <Megaphone aria-hidden="true" />
      <div>
        <strong>{notice.title}</strong>
        {summary && <p>{noticeLine(summary)}</p>}
      </div>
      <div className="announcement-banner-actions">
        <NoticeAction notice={notice} onFollow={onFollow} />
        <button type="button" className="text-button" onClick={onOpenAll}>
          {c.announcementBannerMore}
        </button>
        <button
          type="button"
          className="announcement-banner-close"
          aria-label={c.announcementBannerDismiss}
          title={c.announcementBannerDismiss}
          onClick={() => onDismiss(notice)}
        >
          <X aria-hidden="true" />
        </button>
      </div>
    </section>
  );
}
