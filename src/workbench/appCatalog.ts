import claudeIcon from "../assets/icons/official-claude.png";
import codexIcon from "../assets/icons/official-codex.png";
import piIcon from "../assets/icons/official-pi.svg";
import dshIcon from "../assets/icons/official-dsh.svg";
import opencodeIcon from "../assets/icons/official-opencode.svg";
import workbuddyIcon from "../assets/icons/official-workbuddy.svg";
import type { ActivationToolId } from "../configuration/activation";

/**
 * 工具清单唯一展示源（PRD §3.2 定案）。
 *
 * - `WORKBENCH_APPS`：V1 可接入应用。Claude Code / Codex / Pi / DSH web。
 * - `COMING_SOON_APPS`：标注「即将支持」的应用，只展示、不提供接入入口。
 *
 * 遗留工具（hermes/openclaw）不属于 V1 范围，禁止回到这两个清单
 * （一致性由 appCatalog.test.ts 强校验）。`ACTIVATION_TOOL_IDS`
 * （activation.ts）是 Rust 扫描/接入协议契约，2026-09-14 起与展示层
 * 完全一致（6 个）；增删工具必须先改这里并通过强校验，再同步协议与 Rust。
 */
export const WORKBENCH_APPS: {
  id: ActivationToolId;
  name: string;
  description: string;
  icon?: string;
  mark?: string;
}[] = [
  {
    id: "claude_desktop",
    name: "Claude Desktop",
    description: "Claude 桌面应用",
    icon: claudeIcon,
  },
  {
    id: "codex_desktop",
    name: "Codex Desktop",
    description: "Codex 桌面应用",
    icon: codexIcon,
  },
  {
    id: "workbuddy",
    name: "WorkBuddy",
    description: "腾讯 AI 办公与开发助手",
    icon: workbuddyIcon,
  },
  {
    id: "dsh_web",
    name: "DSH web",
    description: "DeepSeek 浏览器工作台",
    icon: dshIcon,
  },
  { id: "pi", name: "Pi", description: "轻巧的编程助手", icon: piIcon },
  {
    id: "claude_code",
    name: "Claude Code",
    description: "终端与编辑器助手",
    icon: claudeIcon,
  },
];

/** 「即将支持」的应用：仅展示，无适配器、无接入入口（PRD §3.2）。 */
export const COMING_SOON_APPS: {
  id: string;
  name: string;
  description: string;
  icon?: string;
  mark?: string;
}[] = [
  {
    id: "opencode",
    name: "OpenCode",
    description: "开源终端编程助手",
    icon: opencodeIcon,
  },
];
