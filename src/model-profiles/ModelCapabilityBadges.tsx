import { useTranslation } from "react-i18next";
import { modelCapabilities } from "./profile";

const LARGE_CONTEXT_WINDOW_TOKENS = 1_000_000;
const SCALED_CONTEXT_MIN_TOKENS = 100_000;
// Canonical reasoning ladder order. Levels that form a contiguous run are
// rendered as "first–last"; anything else (including "adaptive") falls back
// to an explicit dot-separated list so nothing is implied.
const REASONING_ORDER = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
] as const;

function formatTokens(value: number): string {
  if (value >= LARGE_CONTEXT_WINDOW_TOKENS) {
    return `${roundToTenth(value / LARGE_CONTEXT_WINDOW_TOKENS)}M`;
  }
  if (value >= 1_000) {
    return `${roundToTenth(value / 1_000)}K`;
  }
  return String(value);
}

function roundToTenth(value: number): number {
  return Math.round(value * 10) / 10;
}

function formatReasoningLevels(levels: readonly string[]): string {
  if (levels.length === 1) return levels[0];
  const positions = levels
    .map((level) => REASONING_ORDER.indexOf(level as (typeof REASONING_ORDER)[number]))
    .filter((position) => position >= 0);
  if (
    positions.length === levels.length &&
    new Set(positions).size === positions.length &&
    positions.every(
      (position, index) => index === 0 || position === positions[index - 1] + 1,
    )
  ) {
    return `${levels[0]}–${levels[levels.length - 1]}`;
  }
  return levels.join("·");
}

/**
 * Capability badge row driven by the reviewed reference catalog. Missing
 * fields render no badge — never "unsupported" — and unknown models render
 * nothing at all (capabilities are never guessed, PRD 6.5).
 */
export function ModelCapabilityBadges({ id }: { id: string }) {
  const { t } = useTranslation();
  const capabilities = modelCapabilities(id);
  const badges: string[] = [];
  if (capabilities.contextWindow !== undefined) {
    if (capabilities.contextWindow >= LARGE_CONTEXT_WINDOW_TOKENS) {
      badges.push(t("yeschoyCatalog.capabilities.contextWindow"));
    } else if (capabilities.contextWindow >= SCALED_CONTEXT_MIN_TOKENS) {
      badges.push(
        t("yeschoyCatalog.capabilities.contextWindowScaled", {
          value: formatTokens(capabilities.contextWindow),
        }),
      );
    }
  }
  if (capabilities.input?.includes("image")) {
    badges.push(t("yeschoyCatalog.capabilities.imageInput"));
  }
  if (capabilities.reasoningLevels?.length) {
    badges.push(
      t("yeschoyCatalog.capabilities.reasoningLevels", {
        levels: formatReasoningLevels(capabilities.reasoningLevels),
      }),
    );
    if (capabilities.defaultReasoning) {
      badges.push(
        t("yeschoyCatalog.capabilities.defaultReasoning", {
          level: capabilities.defaultReasoning,
        }),
      );
    }
  }
  if (capabilities.toolUse) {
    badges.push(t("yeschoyCatalog.capabilities.toolUse"));
  }
  if (capabilities.maxOutputTokens !== undefined) {
    badges.push(
      t("yeschoyCatalog.capabilities.maxOutput", {
        value: formatTokens(capabilities.maxOutputTokens),
      }),
    );
  }
  if (!badges.length) return null;
  return (
    <p className="model-capability-badges" aria-label="model-capabilities">
      {badges.map((badge) => (
        <span className="model-capability-badge" key={badge}>
          {badge}
        </span>
      ))}
    </p>
  );
}
