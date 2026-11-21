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

  /** 这个应用客户端能不能自己装（不改动任何东西）。 */
  const peek = useCallback(async (toolId: ActivationToolId) => {
    try {
      const now = await installationAction(toolId, "inspect");
      return now.toolId === toolId ? now.mode : null;
    } catch {
      return null;
    }
  }, []);

  /** 开始装，等到有结果才返回；`null` 是连进度都问不到了。 */
  const run = useCallback(
    async (toolId: ActivationToolId, intent: InstallationIntent) => {
      const mine = ++turn.current;
      const show = (now: InstallationProgress | null) => {
        job.current = now;
        if (turn.current === mine) setProgress(now);
      };
      setStopping(false);
      try {
        let now = await installationAction(toolId, "start", "", intent);
        // 后端一次只装一个：正在装别的应用时，这一个没有开始。
        if (now.toolId !== toolId) return null;
        let misses = 0;
        while (
          installationActive(now) &&
          now.phase !== "awaiting_system_confirmation"
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

  return { progress, stopping, peek, run, stop, guide };
}
