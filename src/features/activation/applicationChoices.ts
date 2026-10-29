import type { ActivationToolId } from "../../configuration/activation";
import type { ConfigurationToolId } from "../../configuration/preview";
import claudeIcon from "../../assets/icons/official-claude.png";
import codexIcon from "../../assets/icons/official-codex.png";
import piIcon from "../../assets/icons/official-pi.svg";
import opencodeIcon from "../../assets/icons/official-opencode.svg";
import dshIcon from "../../assets/icons/official-dsh.svg";

export type ApplicationSurfaceKey =
  | "surfaceClaudeCode"
  | "surfaceClaudeDesktop"
  | "surfaceCodexDesktop"
  | "surfacePi"
  | "surfaceOpenCode"
  | "surfaceDsh"
  | "surfaceDshDesktop"
  | "surfaceWorkBuddy";

export interface ApplicationChoice {
  id: ActivationToolId;
  toolId: ConfigurationToolId;
  displayName: string;
  surface: ApplicationSurfaceKey;
  icon?: string;
  mark?: string;
}

export const APPLICATIONS: readonly ApplicationChoice[] = [
  {
    id: "claude_code",
    toolId: "claude",
    displayName: "Claude Code",
    surface: "surfaceClaudeCode",
    icon: claudeIcon,
  },
  {
    id: "claude_desktop",
    toolId: "claude",
    displayName: "Claude Desktop",
    surface: "surfaceClaudeDesktop",
    icon: claudeIcon,
  },
  {
    id: "codex_desktop",
    toolId: "codex",
    displayName: "Codex",
    surface: "surfaceCodexDesktop",
    icon: codexIcon,
  },
  {
    id: "workbuddy",
    toolId: "workbuddy",
    displayName: "WorkBuddy",
    surface: "surfaceWorkBuddy",
    mark: "W",
  },
  {
    id: "pi",
    toolId: "pi",
    displayName: "Pi",
    surface: "surfacePi",
    icon: piIcon,
  },
  {
    id: "opencode",
    toolId: "opencode",
    displayName: "OpenCode",
    surface: "surfaceOpenCode",
    icon: opencodeIcon,
  },
  {
    id: "dsh_web",
    toolId: "dsh",
    displayName: "DSH web",
    surface: "surfaceDsh",
    icon: dshIcon,
  },
  {
    id: "dsh_desktop",
    toolId: "dsh_desktop",
    displayName: "DeepSeek Harness",
    surface: "surfaceDshDesktop",
    icon: dshIcon,
  },
];
