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

/**
 * 挂在下载源的 `/apps/` 下面，不在主站。
 *
 * 原来指向 `https://yeschoy.com/desktop/model-catalog.json`，那个路径从来没
 * 部署过 —— 主站是个 SPA，任何未知路径都回 200 + HTML，所以这里每次启动都
 * 悄无声息地走 `invalid_payload` 兜底，等于整个功能从没生效过。
 *
 * 下载源（ergou.qzz.io）的 Caddy 是白名单式的，只放行 /apps、/releases、
 * /updates/releases 等几条路径，其余一律 404。`/apps/` 已经在名单里且已经在
 * 服务 catalog.json，所以挂这里不需要改服务器配置 —— 放个文件就行，中转那边
 * 一个字都不用动。
 */
export const REMOTE_MODEL_CATALOG_URL =
  "https://ergou.qzz.io/apps/model-catalog.json";
const REMOTE_CATALOG_SHA256_URL = `${REMOTE_MODEL_CATALOG_URL}.sha256`;
const FETCH_TIMEOUT_MS = 10_000;
const MAX_CATALOG_BYTES = 2 * 1024 * 1024;

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

async function fetchText(url: string): Promise<string | null> {
  try {
    const response = await fetch(url, {
      headers: { accept: "application/json, text/plain" },
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    });
    if (!response.ok) return null;
    const text = await response.text();
    if (!text || text.length > MAX_CATALOG_BYTES) return null;
    return text;
  } catch {
    return null;
  }
}

async function sha256Hex(text: string): Promise<string | null> {
  try {
    const digest = await crypto.subtle.digest(
      "SHA-256",
      new TextEncoder().encode(text),
    );
    return Array.from(new Uint8Array(digest))
      .map((byte) => byte.toString(16).padStart(2, "0"))
      .join("");
  } catch {
    return null;
  }
}

/**
 * Pull the remote catalog and, only if integrity + schema + freshness all
 * pass, hand it to `applyModelCatalogOverride`. Every failure path is a
 * silent fallback to the bundled catalog — this never blocks startup.
 */
export async function refreshModelCatalog(): Promise<RemoteCatalogOutcome> {
  const text = await fetchText(REMOTE_MODEL_CATALOG_URL);
  if (!text) {
    return { status: "bundled", reason: "fetch_failed" };
  }
  const expectedHash = await fetchText(REMOTE_CATALOG_SHA256_URL);
  if (!expectedHash) {
    return { status: "bundled", reason: "hash_unavailable" };
  }
  const digest = await sha256Hex(text);
  if (!digest || !expectedHash.trim().toLowerCase().startsWith(digest)) {
    return { status: "bundled", reason: "hash_mismatch" };
  }
  let parsed: ModelCatalogFile | null;
  try {
    parsed = parseRemoteCatalog(JSON.parse(text));
  } catch {
    return { status: "bundled", reason: "invalid_payload" };
  }
  if (!parsed) {
    return { status: "bundled", reason: "invalid_payload" };
  }
  if (parsed.verifiedAt <= catalog.verifiedAt) {
    return { status: "bundled", reason: "not_newer" };
  }
  applyModelCatalogOverride(parsed.models);
  return {
    status: "updated",
    verifiedAt: parsed.verifiedAt,
    modelCount: parsed.models.length,
  };
}
