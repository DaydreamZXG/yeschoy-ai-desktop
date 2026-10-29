import { useMemo } from "react";
import { balanceAlert, type AccountMoney } from "../../account/finance";
import type { ActivationToolId } from "../../configuration/activation";
import {
  useConnections,
  type ToolConnection,
} from "../../configuration/connections";
import {
  connectionStateUnavailable,
  isSignedIn,
  savedConnectionFor,
} from "./connectionReading";

/** 判读所需的连接状态。共享上下文与测试替身都满足这个形状。 */
export interface ConnectionReadingInput {
  loading?: boolean;
  error?: boolean;
  connections: readonly ToolConnection[];
}

/**
 * 预览页要的账户与连接判读，一处集中。
 *
 * 页面里原本是几行 `?.` 链加布尔组合，散落在组件体内、跨了几百行：想改一条
 * 条件得通读整页才能确定别处没依赖，而这些判读也没有独立的测试 —— 只能靠
 * 渲染出来的界面间接验证。收进 hook 之后页面只拿结果，规则本身由
 * `connectionReading.ts` 的纯函数与其单测守着。
 *
 * 入参只取这个 hook 真正需要的字段（`toolId` 与投影状态），不把整个会话对象
 * 传进来：那样每次会话刷新都会得到新的返回值，调用方的 memo 全部失效。
 */
export function useAccountConnectionReading(input: {
  toolId: ActivationToolId;
  sessionStatus: string | null | undefined;
}) {
  const connections = useConnections();
  return useAccountConnectionReadingWith(connections, input);
}

/**
 * `useAccountConnectionReading` 去掉取共享连接状态那一步：测试和页面里
 * 显式注入连接的路径都把它指向自己的值。
 */
export function useAccountConnectionReadingWith(
  connections: ConnectionReadingInput | null | undefined,
  input: { toolId: ActivationToolId; sessionStatus: string | null | undefined },
) {
  const { toolId, sessionStatus } = input;
  return useMemo(() => {
    const savedConnection = savedConnectionFor(connections, toolId);
    return {
      savedConnection,
      signedIn: isSignedIn(sessionStatus),
      connectionStateUnavailable: connectionStateUnavailable(
        connections
          ? {
              loading: !!connections.loading,
              error: !!connections.error,
              connections: connections.connections,
              saved: savedConnection,
            }
          : null,
      ),
    };
  }, [connections, toolId, sessionStatus]);
}

/**
 * 余额是否值得提醒（偏低或已用尽）。
 *
 * 只在已登录时才有意义：未登录时投影里留着上一次会话的金额，拿它报警会让
 * 「还没登录」的用户先看到「余额不足」。判据本身在 `account/finance`，
 * 这里只负责「未登录一律不报」这一条。
 */
export function useBalanceIssue(
  signedIn: boolean,
  money: AccountMoney | undefined,
) {
  return useMemo(
    () => (signedIn ? balanceAlert(money) : null),
    [signedIn, money],
  );
}
