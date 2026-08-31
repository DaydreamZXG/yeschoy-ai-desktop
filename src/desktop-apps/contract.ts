export type DesktopAppId = "claude_desktop" | "codex_desktop";
export type DesktopAppStatus =
  | "detected_unverified"
  | "not_found"
  | "multiple_installations"
  | "unsupported_platform";

export interface DesktopAppResult {
  appId: DesktopAppId;
  displayName: "Claude Desktop" | "Codex";
  status: DesktopAppStatus;
  version: string;
  candidateCount: number;
  locationHint:
    | "none"
    | "applications"
    | "user_applications"
    | "local_app_data"
    | "program_files"
    | "multiple"
    | "unsupported";
  bundleIdentifier: "" | "com.anthropic.claudefordesktop" | "com.openai.codex";
  configurationStatus: "not_applicable" | "documented_unverified";
  reasonCode:
    | "desktop_app_not_found"
    | "desktop_app_detected_adapter_unverified"
    | "multiple_desktop_apps_found"
    | "desktop_platform_not_supported";
}

export interface DesktopAppScanResponse {
  requestId: string;
  platform: "windows" | "macos" | "linux" | "unknown";
  startedAtEpochMs: number;
  completedAtEpochMs: number;
  apps: DesktopAppResult[];
}

const APP_IDENTITIES = {
  claude_desktop: {
    displayName: "Claude Desktop",
    bundleIdentifier: "com.anthropic.claudefordesktop",
  },
  codex_desktop: {
    displayName: "Codex",
    bundleIdentifier: "com.openai.codex",
  },
} as const;

const APP_IDS = Object.keys(APP_IDENTITIES) as DesktopAppId[];
const STATUSES: DesktopAppStatus[] = [
  "detected_unverified",
  "not_found",
  "multiple_installations",
  "unsupported_platform",
];
const LOCATION_HINTS = [
  "none",
  "applications",
  "user_applications",
  "local_app_data",
  "program_files",
  "multiple",
  "unsupported",
] as const;
const REASONS = [
  "desktop_app_not_found",
  "desktop_app_detected_adapter_unverified",
  "multiple_desktop_apps_found",
  "desktop_platform_not_supported",
] as const;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasOnlyKeys(value: Record<string, unknown>, keys: string[]): boolean {
  const expected = [...keys].sort();
  const actual = Object.keys(value).sort();
  return (
    actual.length === expected.length &&
    actual.every((key, index) => key === expected[index])
  );
}

function isDesktopAppResult(
  value: unknown,
  expectedId: DesktopAppId,
): value is DesktopAppResult {
  if (!isRecord(value)) return false;
  if (
    !hasOnlyKeys(value, [
      "appId",
      "displayName",
      "status",
      "version",
      "candidateCount",
      "locationHint",
      "bundleIdentifier",
      "configurationStatus",
      "reasonCode",
    ]) ||
    value.appId !== expectedId ||
    value.displayName !== APP_IDENTITIES[expectedId].displayName ||
    !STATUSES.includes(value.status as DesktopAppStatus) ||
    typeof value.version !== "string" ||
    value.version.length > 128 ||
    !Number.isInteger(value.candidateCount) ||
    (value.candidateCount as number) < 0 ||
    (value.candidateCount as number) > 8 ||
    !LOCATION_HINTS.includes(
      value.locationHint as (typeof LOCATION_HINTS)[number],
    ) ||
    typeof value.bundleIdentifier !== "string" ||
    !["", APP_IDENTITIES[expectedId].bundleIdentifier].includes(
      value.bundleIdentifier,
    ) ||
    !["not_applicable", "documented_unverified"].includes(
      value.configurationStatus as string,
    ) ||
    !REASONS.includes(value.reasonCode as (typeof REASONS)[number])
  ) {
    return false;
  }

  if (value.status === "detected_unverified") {
    return (
      value.candidateCount === 1 &&
      value.configurationStatus === "documented_unverified" &&
      value.reasonCode === "desktop_app_detected_adapter_unverified"
    );
  }
  if (value.status === "not_found") {
    return (
      value.candidateCount === 0 &&
      value.version === "" &&
      value.bundleIdentifier === "" &&
      value.locationHint === "none" &&
      value.configurationStatus === "not_applicable" &&
      value.reasonCode === "desktop_app_not_found"
    );
  }
  if (value.status === "multiple_installations") {
    return (
      (value.candidateCount as number) > 1 &&
      value.version === "" &&
      value.bundleIdentifier === "" &&
      value.locationHint === "multiple" &&
      value.configurationStatus === "documented_unverified" &&
      value.reasonCode === "multiple_desktop_apps_found"
    );
  }
  return (
    value.candidateCount === 0 &&
    value.version === "" &&
    value.bundleIdentifier === "" &&
    value.locationHint === "unsupported" &&
    value.configurationStatus === "not_applicable" &&
    value.reasonCode === "desktop_platform_not_supported"
  );
}

export function isDesktopAppScanResponse(
  value: unknown,
  requestId: string,
): value is DesktopAppScanResponse {
  if (!isRecord(value)) return false;
  if (
    !hasOnlyKeys(value, [
      "requestId",
      "platform",
      "startedAtEpochMs",
      "completedAtEpochMs",
      "apps",
    ]) ||
    value.requestId !== requestId ||
    !["windows", "macos", "linux", "unknown"].includes(
      value.platform as string,
    ) ||
    !Number.isInteger(value.startedAtEpochMs) ||
    !Number.isInteger(value.completedAtEpochMs) ||
    (value.startedAtEpochMs as number) < 0 ||
    (value.completedAtEpochMs as number) < (value.startedAtEpochMs as number) ||
    !Array.isArray(value.apps) ||
    value.apps.length !== APP_IDS.length
  ) {
    return false;
  }
  const apps = value.apps;
  return APP_IDS.every((appId, index) =>
    isDesktopAppResult(apps[index], appId),
  );
}
