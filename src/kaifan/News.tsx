import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Notice } from "../workbench/announcements";
import { Drawer } from "./Drawer";
import type { RosterId } from "./roster";
import type { Notices } from "./useNotices";

interface Props {
  open: boolean;
  who: RosterId;
  /** 站点现在的情况：连着、没连、连不上。 */
  reach: "ok" | "out" | "off" | "wait";
  notices: Notices;
  onConnect: () => void;
  onClose: () => void;
}

/**
 * 公告栏：站点发的公告，新的标「新」。
 *
 * 打开就算看过了，但这一次打开时是新的那几条，开着的时候一直标着「新」，
 * 不然刚打开标记就没了，等于没标。
 */
export function News({ open, who, reach, notices, onConnect, onClose }: Props) {
  const { t, i18n } = useTranslation();
  const { unread, markSeen } = notices;
  const [fresh, setFresh] = useState<Set<string>>(new Set());
  const [stuck, setStuck] = useState("");
  const all = notices.notices;
  const list = useMemo(() => (reach === "ok" ? all : []), [reach, all]);
  useEffect(() => {
    if (!open) {
      setFresh(new Set());
      setStuck("");
      return;
    }
    // 只有真摆出来的才算看过：站点连不上时列表是空的，不能把没给人看的也记成看过。
    const shown = list.filter((notice) => unread.has(notice.id));
    if (!shown.length) return;
    const ids = shown.map((notice) => notice.id);
    setFresh((current) => new Set([...current, ...ids]));
    markSeen(ids);
  }, [open, list, unread, markSeen]);

  const says =
    reach === "out"
      ? t("kaifan.news.saysOut")
      : reach === "off"
        ? t("kaifan.news.saysOff")
        : reach === "wait"
          ? t("kaifan.news.saysWait")
          : stuck
            ? t("kaifan.news.saysStuck")
            : notices.failed
              ? t("kaifan.news.saysFailed")
              : !notices.available
                ? notices.loading
                  ? t("kaifan.news.saysLoading")
                  : t("kaifan.news.saysNone")
                : !list.length
                  ? t("kaifan.news.saysEmpty")
                  : fresh.size
                    ? t("kaifan.news.saysNew", { count: fresh.size })
                    : t("kaifan.news.saysSeen");
  const day = (at: number) =>
    new Intl.DateTimeFormat(i18n.language, {
      month: "numeric",
      day: "numeric",
    }).format(at);
  const follow = async (notice: Notice) => {
    setStuck("");
    if (!(await notices.follow(notice))) setStuck(notice.id);
  };

  return (
    <Drawer
      open={open}
      testId="kaifan-news"
      label={t("kaifan.nav.news")}
      who={who}
      mood={
        reach !== "ok" || stuck || notices.failed
          ? "panic"
          : list.length
            ? "happy"
            : "idle"
      }
      says={says}
      onClose={onClose}
      foot={
        reach === "out" ? (
          <button type="button" className="go" onClick={onConnect}>
            {t("kaifan.home.outGo")}
          </button>
        ) : reach === "ok" && notices.failed ? (
          <button
            type="button"
            className="go"
            disabled={notices.loading}
            onClick={notices.reload}
          >
            {t("kaifan.news.retry")}
          </button>
        ) : undefined
      }
    >
      {list.length ? (
        <ul className="nws">
          {list.map((notice) => (
            <li
              key={notice.id}
              className={[
                "nw",
                fresh.has(notice.id) ? "new" : "",
                notice.severity === "warning" ? "warn" : "",
              ]
                .join(" ")
                .trim()}
              data-testid="kaifan-notice"
            >
              <header>
                {fresh.has(notice.id) && <i>{t("kaifan.news.tagNew")}</i>}
                <b>{notice.title}</b>
                {notice.publishedAtEpochMs > 0 && (
                  <time
                    dateTime={new Date(notice.publishedAtEpochMs).toISOString()}
                  >
                    {day(notice.publishedAtEpochMs)}
                  </time>
                )}
              </header>
              {notice.body && <p>{notice.body}</p>}
              {(notice.actionLabel || notice.expiresAtEpochMs > 0) && (
                <footer>
                  {notice.expiresAtEpochMs > 0 && (
                    <small>
                      {t("kaifan.news.until", {
                        date: day(notice.expiresAtEpochMs),
                      })}
                    </small>
                  )}
                  {notice.actionLabel && (
                    <button
                      type="button"
                      className="lnk"
                      onClick={() => void follow(notice)}
                    >
                      {notice.actionLabel}
                    </button>
                  )}
                </footer>
              )}
            </li>
          ))}
        </ul>
      ) : (
        <p className="quote">{t("kaifan.news.wouldShow")}</p>
      )}
    </Drawer>
  );
}
