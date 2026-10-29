import { useEffect, useState } from "react";
import { useCelebrateOnRise } from "../../theme/motion";
import {
  markFirstActivationCelebrated,
  shouldCelebrateFirstActivation,
} from "../../configuration/firstActivationMilestone";

export function useActivationCelebration(
  successIsCurrent: boolean,
  firstUseVerified: boolean,
) {
  // 仪式感动效：接入成功后按钮短暂保持成功态，随后回归常态。
  // 条件失效（如外部切换使结果过期）时必须立即结束成功态，
  // 否则 cleanup 只清了定时器，按钮会永远停留在「接入成功」。
  const [applyFlash, setApplyFlash] = useState(false);
  // 一键接入真的写成功的那一刻（applyPhase=finished 且结果是当前选择的），
  // 从浮动条里的按钮位置炸一次彩带 + 对勾。
  useCelebrateOnRise(
    successIsCurrent,
    ".configuration-preview-panel .setup-apply",
  );
  useEffect(() => {
    if (!successIsCurrent) {
      setApplyFlash(false);
      return;
    }
    setApplyFlash(true);
    const timer = window.setTimeout(() => setApplyFlash(false), 1500);
    return () => window.clearTimeout(timer);
  }, [successIsCurrent]);

  // 首次接入庆祝（动效优化）：只有第一次接入成功触发彩带/ticker，
  // 之后的配置成功仍走克制反馈。旗标消费在成功时刻。
  const [firstActivationCelebration, setFirstActivationCelebration] =
    useState(false);
  // 彩带庆祝的是「这东西真的能用」，不是「文件写完了」。所以它等第二级证据，
  // 而不是等 applyPhase。一次性旗标也在这一刻才消费。
  useEffect(() => {
    if (!firstUseVerified) return;
    if (!shouldCelebrateFirstActivation()) return;
    markFirstActivationCelebrated();
    setFirstActivationCelebration(true);
    const timer = window.setTimeout(
      () => setFirstActivationCelebration(false),
      1600,
    );
    return () => window.clearTimeout(timer);
  }, [firstUseVerified]);
  return { applyFlash, firstActivationCelebration };
}
