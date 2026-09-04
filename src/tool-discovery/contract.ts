export const TOOL_CATALOG = [
  { id: "claude", displayName: "Claude Code", mark: "C" },
  { id: "codex", displayName: "Codex", mark: "X" },
  { id: "opencode", displayName: "OpenCode", mark: "O" },
  { id: "pi", displayName: "Pi", mark: "π" },
  { id: "dsh", displayName: "DSH", mark: "D" },
  { id: "hermes", displayName: "Hermes", mark: "H" },
  { id: "openclaw", displayName: "OpenClaw", mark: "O" },
] as const;
export interface ToolResult {
  toolId: (typeof TOOL_CATALOG)[number]["id"];
  displayName: string;
  status:
    | "not_found"
    | "detected_unverified"
    | "probe_failed"
    | "probe_timed_out"
    | "multiple_installations";
  version: string;
  candidateCount: number;
  bundledCount: number;
  selection:
    | "not_found"
    | "bundled_only"
    | "single_installation"
    | "path_precedence"
    | "unresolved";
  locationHint: "none" | "path" | "common_location" | "multiple";
  compatibility: "not_applicable" | "unverified_read_only";
  reasonCode:
    | "tool_not_found"
    | "exact_version_not_allowlisted"
    | "version_command_failed"
    | "version_command_timed_out"
    | "multiple_executables_found";
}
export interface ScanResponse {
  requestId: string;
  platform: "windows" | "macos" | "linux" | "unknown";
  startedAtEpochMs: number;
  completedAtEpochMs: number;
  tools: ToolResult[];
}
function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function keys(value: Record<string, unknown>, expected: string[]) {
  return (
    Object.keys(value).length === expected.length &&
    expected.every((key) => Object.prototype.hasOwnProperty.call(value, key))
  );
}
function count(value: unknown): value is number {
  return Number.isInteger(value) && Number(value) >= 0 && Number(value) <= 32;
}
function timestamp(value: unknown): value is number {
  return (
    Number.isSafeInteger(value) &&
    Number(value) >= 0 &&
    Number(value) <= 8.64e15
  );
}
function tool(value: unknown, index: number): value is ToolResult {
  if (
    !record(value) ||
    !keys(value, [
      "toolId",
      "displayName",
      "status",
      "version",
      "candidateCount",
      "bundledCount",
      "selection",
      "locationHint",
      "compatibility",
      "reasonCode",
    ])
  )
    return false;
  if (
    value.toolId !== TOOL_CATALOG[index].id ||
    value.displayName !== TOOL_CATALOG[index].displayName ||
    !count(value.candidateCount) ||
    !count(value.bundledCount) ||
    typeof value.version !== "string" ||
    value.version.length > 256
  )
    return false;
  const n = value.candidateCount;
  if (n === 0)
    return (
      value.status === "not_found" &&
      value.version === "" &&
      value.locationHint === "none" &&
      value.compatibility === "not_applicable" &&
      value.reasonCode === "tool_not_found" &&
      value.selection ===
        (value.bundledCount > 0 ? "bundled_only" : "not_found")
    );
  if (value.compatibility !== "unverified_read_only") return false;
  if (value.selection === "unresolved")
    return (
      n >= 2 &&
      value.status === "multiple_installations" &&
      value.version === "" &&
      value.locationHint === "multiple" &&
      value.reasonCode === "multiple_executables_found"
    );
  if (
    n >= 32 ||
    (n === 1
      ? value.selection !== "single_installation"
      : value.selection !== "path_precedence")
  )
    return false;
  if (value.locationHint !== "path" && value.locationHint !== "common_location")
    return false;
  if (value.selection === "path_precedence" && value.locationHint !== "path")
    return false;
  switch (value.status) {
    case "detected_unverified":
      return (
        /^\d+\.\d+\.\d[\w.+-]*$/.test(value.version) &&
        value.reasonCode === "exact_version_not_allowlisted"
      );
    case "probe_failed":
      return (
        value.version === "" && value.reasonCode === "version_command_failed"
      );
    case "probe_timed_out":
      return (
        value.version === "" && value.reasonCode === "version_command_timed_out"
      );
    default:
      return false;
  }
}
export function decodeScan(
  value: unknown,
  requestId: string,
): ScanResponse | null {
  if (
    !record(value) ||
    !keys(value, [
      "requestId",
      "platform",
      "startedAtEpochMs",
      "completedAtEpochMs",
      "tools",
    ]) ||
    value.requestId !== requestId ||
    !/^[A-Za-z0-9_-]{1,64}$/.test(requestId)
  )
    return null;
  if (
    !["macos", "windows", "linux", "unknown"].includes(
      String(value.platform),
    ) ||
    !timestamp(value.startedAtEpochMs) ||
    !timestamp(value.completedAtEpochMs) ||
    value.completedAtEpochMs < value.startedAtEpochMs
  )
    return null;
  if (
    !Array.isArray(value.tools) ||
    value.tools.length !== TOOL_CATALOG.length ||
    !value.tools.every(tool)
  )
    return null;
  if (
    value.platform !== "macos" &&
    value.tools.some((item) => item.bundledCount !== 0)
  )
    return null;
  return value as unknown as ScanResponse;
}
