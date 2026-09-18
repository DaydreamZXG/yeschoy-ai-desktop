import { Megaphone, TriangleAlert, RefreshCw } from "lucide-react";
import type { Notice } from "./announcements";
import { useWorkbenchCopy } from "./copy";

function publishedAt(epochMs: number): string {
  if (epochMs <= 0) return "";
  return new Date(epochMs).toLocaleDateString("zh-CN", {
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
                {publishedAt(notice.publishedAtEpochMs) && (
                  <time
                    dateTime={new Date(notice.publishedAtEpochMs).toISOString()}
                  >
                    {publishedAt(notice.publishedAtEpochMs)}
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
