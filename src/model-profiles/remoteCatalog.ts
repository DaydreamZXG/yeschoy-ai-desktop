import { invoke } from "@tauri-apps/api/core";
import catalog from "./catalog.json";
import { applyModelCatalogOverride } from "./profile";

/**
 * Remote model-capability catalog refresh (batch 2 / M5).
 *
 * The reviewed catalog ships with the app and remains the fallback. At
 * startup we try to pull a newer revision from the official static host and
 * only apply it after a sha256 integrity check and schema validation —
 * any failure keeps the bundled catalog (client-autonomous, PRD 6.5:
 * catalog updates must never require a NewAPI server change or a release).
 */

export type BundledCatalogModel = (typeof catalog.models)[number];

export interface ModelCatalogFile {
  schemaVersion: number;
  verifiedAt: string;
  models: BundledCatalogModel[];
}

export type RemoteCatalogOutcome =
  | { status: "updated"; verifiedAt: string; modelCount: number }
  | {
      status: "bundled";
      reason:
        | "fetch_failed"
        | "hash_unavailable"
        | "hash_mismatch"
        | "invalid_payload"
        | "not_newer";
    };

const INPUT_MODALITIES = new Set(["text", "image"]);

function isPositiveInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value > 0;
}

function isHttpUrl(value: unknown): value is string {
  if (typeof value !== "string") return false;
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

/**
 * Validate an untrusted remote payload against the catalog schema. Returns
 * null when anything is off — callers then keep the bundled catalog instead
 * of guessing (PRD 6.5: unknown or malformed data never reaches the UI).
 */
export function parseRemoteCatalog(payload: unknown): ModelCatalogFile | null {
  if (!payload || typeof payload !== "object") return null;
  const value = payload as Record<string, unknown>;
  if (value.schemaVersion !== catalog.schemaVersion) return null;
  if (
    typeof value.verifiedAt !== "string" ||
    !/^\d{4}-\d{2}-\d{2}$/.test(value.verifiedAt)
  ) {
    return null;
  }
  if (!Array.isArray(value.models) || value.models.length === 0) return null;
  const seen = new Set<string>();
  for (const model of value.models) {
    if (!model || typeof model !== "object") return null;
    const entry = model as Record<string, unknown>;
    if (typeof entry.id !== "string" || !entry.id || seen.has(entry.id)) {
      return null;
    }
    seen.add(entry.id);
    if (typeof entry.displayName !== "string" || !entry.displayName) {
      return null;
    }
    if (
      entry.contextWindow !== undefined &&
      !isPositiveInteger(entry.contextWindow)
    ) {
      return null;
    }
    if (
      entry.maxOutputTokens !== undefined &&
      !isPositiveInteger(entry.maxOutputTokens)
    ) {
      return null;
    }
    if (
      entry.input !== undefined &&
      (!Array.isArray(entry.input) ||
        entry.input.length === 0 ||
        !entry.input.every(
          (item) =>
            typeof item === "string" && INPUT_MODALITIES.has(item as string),
        ) ||
        !entry.input.includes("text"))
    ) {
      return null;
    }
    if (
      entry.reasoningLevels !== undefined &&
      (!Array.isArray(entry.reasoningLevels) ||
        !entry.reasoningLevels.every(
          (level) => typeof level === "string" && level,
        ))
    ) {
      return null;
    }
    if (
      entry.defaultReasoning !== undefined &&
      (typeof entry.defaultReasoning !== "string" || !entry.defaultReasoning)
    ) {
      return null;
    }
    if (
      entry.reasoningMode !== undefined &&
      (typeof entry.reasoningMode !== "string" || !entry.reasoningMode)
    ) {
      return null;
    }
    if (entry.toolUse !== undefined && typeof entry.toolUse !== "boolean") {
      return null;
    }
    if (
      !Array.isArray(entry.sources) ||
      entry.sources.length === 0 ||
      !entry.sources.every(isHttpUrl)
    ) {
      return null;
    }
  }
  return {
    schemaVersion: value.schemaVersion as number,
    verifiedAt: value.verifiedAt as string,
    models: value.models as BundledCatalogModel[],
  };
}

const FALLBACK_REASONS = new Set([
  "fetch_failed",
  "hash_unavailable",
  "hash_mismatch",
  "invalid_payload",
  "not_newer",
]);

/**
 * Ask the native side to refresh the catalog, and adopt what it adopted.
 *
 * The fetch used to happen here, with the webview's own `fetch`. It could
 * never succeed: the app's CSP allows `connect-src 'self' ipc:
 * http://ipc.localhost` and nothing else, so the request to the download host
 * was blocked before it left the process and this function always reported
 * `fetch_failed`. Pointing it at a URL that was actually deployed — an earlier
 * round did exactly that — changed nothing, because the URL was never the only
 * thing in the way.
 *
 * It also would not have been enough. `applyModelCatalogOverride` swaps a map
 * that lives on this side of the IPC boundary, while Claude Code's
 * `autoCompactWindow` is written from the native catalog. A refresh that only
 * landed here left auto-compact on whatever shipped in the binary.
 *
 * So the native side fetches, verifies (sha256, schema, strictly newer
 * `verifiedAt`) and caches, and hands back the payload it accepted. Re-parsing
 * it here is deliberate: it is cheap, it keeps this module's validator the
 * single description of the schema the picker relies on, and the two sides
 * cannot silently disagree about what they adopted.
 */
export async function refreshModelCatalog(): Promise<RemoteCatalogOutcome> {
  let raw: unknown;
  try {
    raw = await invoke("refresh_model_catalog_v1", {
      requestId: `catalog-${Date.now().toString(36)}`,
    });
  } catch {
    return { status: "bundled", reason: "fetch_failed" };
  }
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
    return { status: "bundled", reason: "invalid_payload" };
  }
  const value = raw as Record<string, unknown>;
  if (value.schemaVersion !== 1 || typeof value.reasonCode !== "string") {
    return { status: "bundled", reason: "invalid_payload" };
  }
  if (value.status !== "updated") {
    return {
      status: "bundled",
      reason: FALLBACK_REASONS.has(value.reasonCode)
        ? (value.reasonCode as Exclude<
            RemoteCatalogOutcome,
            { status: "updated" }
          >["reason"])
        : "invalid_payload",
    };
  }
  const parsed = parseRemoteCatalog(value.catalog);
  if (!parsed || parsed.verifiedAt <= catalog.verifiedAt) {
    return { status: "bundled", reason: "invalid_payload" };
  }
  applyModelCatalogOverride(parsed.models);
  return {
    status: "updated",
    verifiedAt: parsed.verifiedAt,
    modelCount: parsed.models.length,
  };
}
