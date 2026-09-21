import { Megaphone, TriangleAlert, RefreshCw } from "lucide-react";
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

export function AnnouncementsView({
  notices,
  loading,
  failed,
  onRetry,
}: {
  notices: Notice[];
  loading: boolean;
  failed: boolean;
  onRetry: () => void;
}) {
  const c = useWorkbenchCopy();
  const { i18n } = useTranslation();
  const locale = i18n.language;
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

      {failed ? (
        <p className="workbench-notice" role="status">
          <TriangleAlert aria-hidden="true" />
          {c.announcementsFailed}
        </p>
      ) : notices.length === 0 && !loading ? (
        <p className="announcements-empty">
          <Megaphone aria-hidden="true" />
          {c.announcementsEmpty}
        </p>
      ) : (
        <ol className="announcement-list">
          {notices.map((notice) => (
            <li
              key={notice.id}
              className="announcement-card"
              data-severity={notice.severity}
            >
              <div className="announcement-heading">
                <h2>{notice.title}</h2>
                {publishedAt(notice.publishedAtEpochMs, locale) && (
                  <time
                    dateTime={new Date(notice.publishedAtEpochMs).toISOString()}
                  >
                    {publishedAt(notice.publishedAtEpochMs, locale)}
                  </time>
                )}
              </div>
              {/* 服务端自由文本，只按换行分段渲染，绝不当成标记解析。 */}
              {notice.body
                .split("\n")
                .filter((line) => line.trim().length > 0)
                .map((line, index) => (
                  <p key={index}>{line}</p>
                ))}
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
