import claudeIcon from "../assets/icons/official-claude.png";
import clineIcon from "../assets/icons/official-cline.png";
import hermesIcon from "../assets/icons/official-hermes.png";
import openclawIcon from "../assets/icons/official-openclaw.png";
import codexIcon from "../assets/icons/official-codex.png";
import piIcon from "../assets/icons/official-pi.svg";
import dshIcon from "../assets/icons/official-dsh.svg";
import opencodeIcon from "../assets/icons/official-opencode.svg";
import workbuddyIcon from "../assets/icons/official-workbuddy.svg";
import cursorppIcon from "../assets/icons/cursorpp.svg";
import type { ActivationToolId } from "../configuration/activation";

/**
 * 工具清单唯一展示源（PRD §3.2 定案）。
 *
 * - `WORKBENCH_APPS`：已支持接入的应用。Claude Code / Claude Desktop / Codex / WorkBuddy / DeepSeek Harness / Pi / OpenCode / Cline / Hermes Agent / OpenClaw。
 * - `COMING_SOON_APPS`：标注「即将支持」的应用，只展示、不提供接入入口。
 *
 * 这份清单和 `ACTIVATION_TOOL_IDS`（activation.ts，Rust 扫描/接入协议契约）
 * 必须完全一致，由 appCatalog.test.ts 强校验；增删工具先改这里并通过强校验，
 * 再同步协议与 Rust。
 */
export const WORKBENCH_APPS: {
  id: ActivationToolId;
  name: string;
  icon?: string;
  mark?: string;
  /** 常用的默认露在外面；不常用的收在「还有几个」里。 */
  common: boolean;
}[] = [
  {
    id: "claude_desktop",
    name: "Claude Desktop",
    icon: claudeIcon,
    common: true,
  },
  {
    id: "codex_desktop",
    name: "Codex",
    icon: codexIcon,
    common: true,
  },
  { id: "opencode", name: "OpenCode", icon: opencodeIcon, common: true },
  {
    id: "claude_code",
    name: "Claude Code",
    icon: claudeIcon,
    common: true,
  },
  {
    id: "workbuddy",
    name: "WorkBuddy",
    icon: workbuddyIcon,
    common: true,
  },
  { id: "pi", name: "Pi", icon: piIcon, common: true },
  {
    id: "dsh_desktop",
    name: "DeepSeek Harness",
    icon: dshIcon,
    common: false,
  },
  { id: "cline", name: "Cline", icon: clineIcon, common: false },
  { id: "hermes", name: "Hermes Agent", icon: hermesIcon, common: false },
  { id: "openclaw", name: "OpenClaw", icon: openclawIcon, common: false },
  { id: "cursorpp", name: "Cursor++", icon: cursorppIcon, common: false },
];

/** 「即将支持」的应用：仅展示，无适配器、无接入入口（PRD §3.2）。 */
export const COMING_SOON_APPS: {
  id: string;
  name: string;
  icon?: string;
  mark?: string;
}[] = [];

/** 界面上的应用名。少数名字要带小白话说明，走 i18n，避免源码里写死中文。 */
export function appDisplayName(
  appId: string,
  fallback: string,
  t: (key: string) => string,
): string {
  if (appId === "codex_desktop") return t("workbench.appNameCodex");
  return fallback;
}

/**
 * 不再让人新接的应用。DSH 改成桌面应用（`dsh_desktop`）以后网页版退役了：
 * 名单里不列它，也不能新接；以前接过的照样露在首页，能「放回去」。
 */
export const RETIRED_APPS = [
  {
    id: "dsh_web",
    name: "DSH web",
    icon: dshIcon,
    common: false,
  },
] as const satisfies readonly {
  id: ActivationToolId;
  name: string;
  icon: string;
  common: boolean;
}[];

export function isRetiredApp(appId: string): boolean {
  return RETIRED_APPS.some((app) => app.id === appId);
}

/** 按 id 找应用，退役的也找得到：已经接过的还要显示名字。 */
export function findApp(appId: string) {
  return [...WORKBENCH_APPS, ...RETIRED_APPS].find((app) => app.id === appId);
}

/** 首页放得下的那几个：接过的排前面，然后是常用的，同一档里保持名单顺序。 */
export function appsForHome(linked: readonly string[], limit: number) {
  const apps = [
    ...WORKBENCH_APPS,
    ...RETIRED_APPS.filter((app) => linked.includes(app.id)),
  ];
  const rank = (app: (typeof apps)[number]) =>
    (linked.includes(app.id) ? 2 : 0) + (app.common ? 1 : 0);
  return apps.sort((a, b) => rank(b) - rank(a)).slice(0, limit);
}
