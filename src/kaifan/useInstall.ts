/**
 * 安装一个还没装的应用，并一直看着它装完。
 *
 * 下载、核对、放到电脑上都是后端在做；这里只负责开始、每隔一会儿问一次进度、
 * 以及用户说停的时候去停。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import type { ActivationToolId } from "../configuration/activation";
import {
  installationAction,
  installationActive,
  type InstallationIntent,
  type InstallationProgress,
} from "../installation/api";

const EVERY_MS = 900;
/** 连着这么多次问不到进度，就当它没了。 */
const MAX_MISSES = 6;
/** 点完「装好了」之后，最多等这么多次看后端开始核对。 */
const SETTLE_TRIES = 4;

export function useInstall() {
  const [progress, setProgress] = useState<InstallationProgress | null>(null);
  const [stopping, setStopping] = useState(false);
  const turn = useRef(0);
  const job = useRef<InstallationProgress | null>(null);
  useEffect(
    () => () => {
      turn.current = -1;
    },
    [],
  );

  /** 这个应用现在装到哪了、客户端能不能自己装（只看，不改动任何东西）。 */
  const peek = useCallback(async (toolId: ActivationToolId) => {
    try {
      const now = await installationAction(toolId, "inspect");
      return now.toolId === toolId ? now : null;
    } catch {
      return null;
    }
  }, []);

  /**
   * 看着一次安装走到停下来的地方：装好了、没装成、被停下，或者轮到系统的安装
   * 窗口、要等用户在那边点完。`null` 是连进度都问不到了。
   */
  const watch = useCallback(
    async (
      toolId: ActivationToolId,
      begin: () => Promise<InstallationProgress>,
      /** 刚点完「装好了」：后端要过一下才开始查，头几次还停在原地不算数。 */
      settling = false,
    ) => {
      const mine = ++turn.current;
      const show = (now: InstallationProgress | null) => {
        job.current = now;
        if (turn.current === mine) setProgress(now);
      };
      setStopping(false);
      try {
        let now = await begin();
        // 后端一次只装一个：正在装别的应用时，这一个没有开始。
        if (now.toolId !== toolId) return null;
        let misses = 0;
        let idle = settling ? SETTLE_TRIES : 0;
        while (
          installationActive(now) &&
          (now.phase !== "awaiting_system_confirmation" ||
            (idle-- > 0 && now.reasonCode === "none"))
        ) {
          show(now);
          await new Promise((done) => window.setTimeout(done, EVERY_MS));
          if (turn.current !== mine) return null;
          try {
            now = await installationAction(toolId, "inspect", now.jobId);
            misses = 0;
          } catch {
            if (++misses >= MAX_MISSES) return null;
          }
        }
        return now;
      } catch {
        return null;
      } finally {
        show(null);
      }
    },
    [],
  );

  /** 开始装，等到有结果才返回。 */
  const run = useCallback(
    (toolId: ActivationToolId, intent: InstallationIntent) =>
      watch(toolId, () => installationAction(toolId, "start", "", intent)),
    [watch],
  );

  /** 用户在系统的安装窗口里点完了：去核对装好没有。 */
  const confirm = useCallback(
    (waiting: InstallationProgress, intent: InstallationIntent) =>
      watch(
        waiting.toolId,
        () =>
          installationAction(waiting.toolId, "confirm", waiting.jobId, intent),
        true,
      ),
    [watch],
  );

  /** 系统的安装窗口被关掉了：再打开一次。 */
  const reopen = useCallback((waiting: InstallationProgress) => {
    void installationAction(waiting.toolId, "reopen", waiting.jobId).catch(
      () => undefined,
    );
  }, []);

  /** 不等了。系统那边已经开始的安装不归我们管，不会被取消。 */
  const drop = useCallback(async (waiting: InstallationProgress) => {
    await installationAction(waiting.toolId, "cancel", waiting.jobId).catch(
      () => undefined,
    );
  }, []);

  const stop = useCallback(async () => {
    const now = job.current;
    if (!now?.canCancel) return;
    setStopping(true);
    try {
      await installationAction(now.toolId, "cancel", now.jobId);
    } catch {
      setStopping(false);
    }
  }, []);

  /** 打开这个应用的官方下载页，让用户自己装。 */
  const guide = useCallback((toolId: ActivationToolId) => {
    void installationAction(toolId, "help").catch(() => undefined);
  }, []);

  return { progress, stopping, peek, run, confirm, reopen, drop, stop, guide };
}
