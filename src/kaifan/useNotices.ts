import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  followAnnouncement,
  readAnnouncements,
  readSeenAnnouncements,
  rememberSeenAnnouncements,
  type Notice,
} from "../workbench/announcements";

const REFRESH_MS = 30 * 60 * 1000;
const FOCUS_REFRESH_MS = 5 * 60 * 1000;

export interface Notices {
  /** 这个站有没有公告栏。 */
  available: boolean;
  notices: Notice[];
  loading: boolean;
  /** 这次没读到：之前读到过，或者原生侧说是断网、站点出错。可以再读一次。 */
  failed: boolean;
  /** 还没看过的公告。 */
  unread: Set<string>;
  reload: () => void;
  markSeen: (ids: string[]) => void;
  /** 点公告上的按钮；没打开成返回 false。 */
  follow: (notice: Notice) => Promise<boolean>;
}

const NONE = { available: false, notices: [], loading: false, failed: false };

/** 站点公告：连上就读，半小时读一次，回到窗口超过五分钟再读一次。 */
export function useNotices(lineId: string, signedIn: boolean): Notices {
  const [state, setState] =
    useState<Pick<Notices, "available" | "notices" | "loading" | "failed">>(
      NONE,
    );
  const readAt = useRef(0);
  const turn = useRef(0);
  const load = useCallback(async () => {
    const mine = ++turn.current;
    readAt.current = Date.now();
    setState((current) => ({ ...current, loading: true, failed: false }));
    const result = await readAnnouncements(lineId);
    // 换了站点或者又读了一次：这一次的结果不要了。
    if (mine !== turn.current) return;
    setState((current) => ({
      // 读到过公告栏之后，一次没读到不算「这个站没有公告栏」，只算这次没读到。
      available: result.available || current.available,
      notices: result.available ? result.notices : current.notices,
      loading: false,
      failed: !result.available && (current.available || !!result.missed),
    }));
  }, [lineId]);

  useEffect(() => {
    turn.current += 1;
    setState(NONE);
    if (!signedIn) return;
    void load();
    const timer = window.setInterval(() => void load(), REFRESH_MS);
    const onVisible = () => {
      if (
        document.visibilityState === "visible" &&
        Date.now() - readAt.current > FOCUS_REFRESH_MS
      )
        void load();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [signedIn, load]);

  const [seen, setSeen] = useState(readSeenAnnouncements);
  const unread = useMemo(
    () =>
      new Set(
        state.notices.map((notice) => notice.id).filter((id) => !seen.has(id)),
      ),
    [state.notices, seen],
  );
  const markSeen = useCallback((ids: string[]) => {
    setSeen((current) =>
      ids.every((id) => current.has(id))
        ? current
        : rememberSeenAnnouncements(current, ids),
    );
  }, []);
  const follow = useCallback(
    (notice: Notice) => {
      markSeen([notice.id]);
      return followAnnouncement(lineId, notice.id);
    },
    [lineId, markSeen],
  );

  return {
    ...state,
    unread,
    reload: () => void load(),
    markSeen,
    follow,
  };
}
