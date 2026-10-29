import type {
  ConnectivityLineResult,
  ConnectivityResponse,
  LineOutcome,
} from "./contract";
import { lineOutcome } from "./contract";

/**
 * Health assistant + bug report payloads.
 *
 * The renderer is the last stop before something leaves the user's machine,
 * so it re-checks the native snapshot instead of trusting it: the native side
 * already scrubs the log, but a future command change must not turn this into
 * a leak. Anything unexpected makes the whole snapshot invalid rather than
 * "best effort" — a report with half-verified fields is worse than a retry.
 */
export interface HealthMarker {
  element: string;
  detail: string;
}

export interface HealthSnapshot {
  platform: "macos" | "windows" | "linux" | "unknown";
  arch: string;
  appVersion: string;
  frontendMode: "bundled-custom-protocol" | "dev-server";
  distribution: "community" | "official" | "partner";
  logAvailable: boolean;
  logTail: string[];
  markers: HealthMarker[];
}

const PLATFORMS = ["macos", "windows", "linux", "unknown"] as const;
const FRONTEND_MODES = ["bundled-custom-protocol", "dev-server"] as const;
const DISTRIBUTIONS = ["community", "official", "partner"] as const;

/** Backstop mirrors `health_snapshot.rs`: no path, URL, IP or token shapes. */
const LEAK_PATTERNS = [
  /:\/\//,
  /\/(?:Users|home|root|private|var|etc|tmp|opt|Applications)\//,
  /[A-Za-z]:\\/,
  /\b(?:\d{1,3}\.){3}\d{1,3}\b/,
  /\bsk-[A-Za-z0-9]/,
  /\beyJ[A-Za-z0-9_-]{10,}/, // JWT
];

export function containsSensitiveText(text: string): boolean {
  return LEAK_PATTERNS.some((pattern) => pattern.test(text));
}

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function exactKeys(
  value: Record<string, unknown>,
  keys: readonly string[],
): boolean {
  const actual = Object.keys(value);
  return (
    actual.length === keys.length && keys.every((key) => actual.includes(key))
  );
}

function shortText(value: unknown, max: number): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= max;
}

export function decodeHealthSnapshot(value: unknown): HealthSnapshot | null {
  if (
    !record(value) ||
    !exactKeys(value, [
      "platform",
      "arch",
      "appVersion",
      "frontendMode",
      "distribution",
      "logAvailable",
      "logTail",
      "markers",
    ])
  )
    return null;
  if (!(PLATFORMS as readonly unknown[]).includes(value.platform)) return null;
  if (!(FRONTEND_MODES as readonly unknown[]).includes(value.frontendMode))
    return null;
  if (!(DISTRIBUTIONS as readonly unknown[]).includes(value.distribution))
    return null;
  if (!shortText(value.arch, 40) || !shortText(value.appVersion, 40))
    return null;
  if (typeof value.logAvailable !== "boolean") return null;
  if (!Array.isArray(value.logTail) || value.logTail.length > 200) return null;
  if (
    value.logTail.some((line) => typeof line !== "string" || line.length > 400)
  )
    return null;
  if (!Array.isArray(value.markers) || value.markers.length > 32) return null;
  if (
    value.markers.some(
      (marker) =>
        !record(marker) ||
        !exactKeys(marker, ["element", "detail"]) ||
        !shortText(marker.element, 80) ||
        typeof marker.detail !== "string" ||
        marker.detail.length > 240,
    )
  )
    return null;

  const snapshot = value as unknown as HealthSnapshot;
  if (
    snapshot.logTail.some(containsSensitiveText) ||
    snapshot.markers.some(
      (marker) =>
        containsSensitiveText(marker.element) ||
        containsSensitiveText(marker.detail),
    )
  )
    return null;
  return {
    ...snapshot,
    logTail: [...snapshot.logTail],
    markers: [...snapshot.markers],
  };
}

/** One line per known failure, in check order, so the reader sees the first cause. */
export interface HealthFinding {
  lineId: ConnectivityResponse["lines"][number]["lineId"];
  layer: ConnectivityLineResult["layers"][number]["layer"];
  reasonCode: ConnectivityLineResult["layers"][number]["reasonCode"];
  outcome: LineOutcome;
}

export function healthFindings(
  response: ConnectivityResponse,
): HealthFinding[] {
  const findings: HealthFinding[] = [];
  for (const line of response.lines) {
    if (line.layers.every((layer) => layer.status !== "failed")) continue;
    const first = line.layers.find((layer) => layer.status === "failed");
    if (!first) continue;
    findings.push({
      lineId: line.lineId,
      layer: first.layer,
      reasonCode: first.reasonCode,
      outcome: lineOutcome(line),
    });
  }
  return findings;
}

export interface ReportInput {
  generatedAt: Date;
  snapshot: HealthSnapshot | null;
  connectivity: ConnectivityResponse | null;
  /** Free-text the user typed; quoted verbatim, never evaluated. */
  userNote: string;
}

const MAX_NOTE_CHARS = 2000;

/**
 * Build the single text a user is asked to paste to the developer.
 *
 * Deliberately English on the content lines (reason codes and stage markers
 * are already stable English tokens on both ends of the stack) while section
 * titles stay neutral; the surrounding UI is localized. Ordering is fixed so
 * two reports can be diffed.
 */
export function buildHealthReport(input: ReportInput): string {
  const { generatedAt, snapshot, connectivity } = input;
  const lines: string[] = ["Yeschoy health report (schema v1)"];
  lines.push(`generated-at: ${generatedAt.toISOString()}`);
  if (snapshot) {
    lines.push(
      `env: platform=${snapshot.platform} arch=${snapshot.arch} app=${snapshot.appVersion} frontend=${snapshot.frontendMode} distribution=${snapshot.distribution}`,
    );
  } else {
    lines.push("env: unavailable");
  }

  lines.push("", "[connectivity]");
  if (!connectivity || connectivity.lines.length === 0) {
    lines.push("not-checked");
  } else {
    for (const line of connectivity.lines) {
      const outcome = lineOutcome(line);
      lines.push(`${line.lineId}: ${outcome}`);
      for (const layer of line.layers) {
        const latency =
          layer.latencyMs !== undefined ? ` ${layer.latencyMs}ms` : "";
        lines.push(
          `  ${layer.layer} ${layer.status}${latency} ${layer.reasonCode}`,
        );
      }
    }
  }

  lines.push("", "[stage-markers]");
  if (snapshot) {
    if (snapshot.markers.length === 0) lines.push("none");
    for (const marker of snapshot.markers)
      lines.push(`${marker.element} ${marker.detail}`.trim());
  } else {
    lines.push("unavailable");
  }

  lines.push("", "[log-tail]");
  if (!snapshot) {
    lines.push("unavailable");
  } else if (!snapshot.logAvailable) {
    lines.push("log-not-available");
  } else if (snapshot.logTail.length === 0) {
    lines.push("empty");
  } else {
    lines.push(...snapshot.logTail);
  }

  const note = input.userNote.trim().slice(0, MAX_NOTE_CHARS);
  lines.push("", "[what-happened]");
  lines.push(note.length > 0 ? note : "none");

  return lines.join("\n");
}
