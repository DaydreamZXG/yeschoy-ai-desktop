import { OFFICIAL_SITE_ORIGIN } from "../distribution/officialSitePromo";
import { uiDistribution } from "../distribution/identity";

/**
 * Where "反馈 Bug" sends a user, per distribution channel.
 *
 * The community build must only ever point at the public repository — the
 * official helper site is compiled into official packages only (see
 * docs/BRANDING.md). Both entries are plain links opened in the system
 * browser; this app never uploads anything on the user's behalf.
 */
export interface FeedbackChannel {
  /** `community` goes to GitHub issues, `official` to the help page. */
  id: "community" | "official";
  url: string;
}

export function feedbackChannel(): FeedbackChannel {
  if (uiDistribution() === "official")
    return { id: "official", url: `${OFFICIAL_SITE_ORIGIN}/feedback` };
  return {
    id: "community",
    url: "https://github.com/DaydreamZXG/yeschoy-ai-desktop/issues/new",
  };
}

/**
 * Strip anything that looks like a credential from text the user copies.
 *
 * Reports are user-editable before they leave (the textarea), so someone may
 * paste a key back in by accident while adding detail. Redacting on copy —
 * not on input — keeps what they typed visible on screen.
 */
const SECRET_PATTERNS: [RegExp, string][] = [
  [/sk-[A-Za-z0-9_-]{8,}/g, "sk-[redacted]"],
  [
    /\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{5,}\b/g,
    "[redacted-jwt]",
  ],
  [/Bearer\s+[A-Za-z0-9._~+/-]{10,}=*/g, "Bearer [redacted]"],
  [
    /\b(api[_-]?key|token|secret|password)\b(\s*[:=]\s*)("[^"]*"|'[^']*'|\S+)/gi,
    "$1$2[redacted]",
  ],
];

export function redactSecrets(text: string): string {
  return SECRET_PATTERNS.reduce(
    (current, [pattern, replacement]) => current.replace(pattern, replacement),
    text,
  );
}
