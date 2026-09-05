import claudeIcon from "../assets/icons/claude.svg";
import codexIcon from "../assets/icons/chatgpt.svg";
import type { ActivationToolId } from "../configuration/activation";
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
    id: "dsh_web",
    name: "DSH web",
    description: "DeepSeek 浏览器工作台",
    mark: "D",
  },
  { id: "pi", name: "Pi", description: "轻巧的编程助手", mark: "π" },
  {
    id: "claude_code",
    name: "Claude Code",
    description: "终端与编辑器助手",
    icon: claudeIcon,
  },
  {
    id: "hermes",
    name: "Hermes",
    description: "爱马仕 · 个人 AI 助手",
    mark: "H",
  },
  {
    id: "openclaw",
    name: "OpenClaw",
    description: "小龙虾 · 智能助手",
    mark: "O",
  },
];
