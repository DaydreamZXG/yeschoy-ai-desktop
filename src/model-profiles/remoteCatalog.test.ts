import { afterEach, describe, expect, it, vi } from "vitest";
import catalog from "./catalog.json";
import { applyModelCatalogOverride } from "./profile";
import {
  parseRemoteCatalog,
  refreshModelCatalog,
  REMOTE_MODEL_CATALOG_URL,
  type BundledCatalogModel,
} from "./remoteCatalog";

function validRemoteCatalog(overrides: Partial<unknown> = {}) {
  return {
    schemaVersion: catalog.schemaVersion,
    verifiedAt: "2099-01-01",
    models: [
      {
        id: "future-model",
        displayName: "Future Model",
        reasoningLevels: ["low", "high"],
        input: ["text"],
        contextWindow: 1000000,
        maxOutputTokens: 128000,
        sources: ["https://example.com/future-model"],
      },
    ] as BundledCatalogModel[],
    ...overrides,
  };
}

function mockFetch(responses: Record<string, { ok?: boolean; text?: string }>) {
  return vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string | URL) => {
      const response = responses[String(url)];
      if (!response) throw new Error("network");
      return {
        ok: response.ok !== false,
        text: async () => response.text ?? "",
      } as Response;
    }),
  );
}

async function sha256(text: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(text),
  );
  return Array.from(new Uint8Array(digest))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

afterEach(() => {
  vi.unstubAllGlobals();
  applyModelCatalogOverride(null);
});

describe("parseRemoteCatalog validates untrusted payloads", () => {
  it("accepts a well-formed catalog", () => {
    expect(parseRemoteCatalog(validRemoteCatalog())).not.toBeNull();
  });

  it("rejects unknown schema versions, bad dates, and empty models", () => {
    expect(parseRemoteCatalog(validRemoteCatalog({ schemaVersion: 2 }))).toBeNull();
    expect(
      parseRemoteCatalog(validRemoteCatalog({ verifiedAt: "not-a-date" })),
    ).toBeNull();
    expect(parseRemoteCatalog(validRemoteCatalog({ models: [] }))).toBeNull();
  });

  it("rejects duplicate ids and malformed capability fields", () => {
    const duplicate = validRemoteCatalog();
    duplicate.models.push({ ...duplicate.models[0] });
    expect(parseRemoteCatalog(duplicate)).toBeNull();

    const badContext = validRemoteCatalog();
    badContext.models[0].contextWindow = -1;
    expect(parseRemoteCatalog(badContext)).toBeNull();

    const badInput = validRemoteCatalog();
    badInput.models[0].input = ["audio"];
    expect(parseRemoteCatalog(badInput)).toBeNull();

    const badSources = validRemoteCatalog();
    badSources.models[0].sources = ["not-a-url"];
    expect(parseRemoteCatalog(badSources)).toBeNull();
  });

  it("accepts the optional toolUse/defaultReasoning/reasoningMode fields", () => {
    const enriched = validRemoteCatalog();
    enriched.models[0] = {
      ...enriched.models[0],
      toolUse: true,
      defaultReasoning: "high",
      reasoningMode: "deepseek",
    } as BundledCatalogModel;
    expect(parseRemoteCatalog(enriched)).not.toBeNull();
  });

  it("rejects malformed optional capability fields", () => {
    const badToolUse = validRemoteCatalog();
    badToolUse.models[0] = {
      ...badToolUse.models[0],
      toolUse: "yes",
    } as unknown as BundledCatalogModel;
    expect(parseRemoteCatalog(badToolUse)).toBeNull();

    const badDefault = validRemoteCatalog();
    badDefault.models[0] = {
      ...badDefault.models[0],
      defaultReasoning: "",
    } as BundledCatalogModel;
    expect(parseRemoteCatalog(badDefault)).toBeNull();

    const badMode = validRemoteCatalog();
    badMode.models[0] = {
      ...badMode.models[0],
      reasoningMode: 42,
    } as unknown as BundledCatalogModel;
    expect(parseRemoteCatalog(badMode)).toBeNull();
  });
});

describe("refreshModelCatalog falls back to the bundled catalog", () => {
  it("applies a newer, hash-verified revision", async () => {
    const body = JSON.stringify(validRemoteCatalog());
    mockFetch({
      [REMOTE_MODEL_CATALOG_URL]: { text: body },
      [`${REMOTE_MODEL_CATALOG_URL}.sha256`]: { text: await sha256(body) },
    });
    const outcome = await refreshModelCatalog();
    expect(outcome).toEqual({
      status: "updated",
      verifiedAt: "2099-01-01",
      modelCount: 1,
    });
    expect(await import("./profile")).toMatchObject({
      modelCapabilities: expect.any(Function),
    });
    expect((await import("./profile")).modelCapabilities("future-model"))
      .toMatchObject({ contextWindow: 1000000 });
  });

  it("keeps the bundled catalog when the network fails", async () => {
    mockFetch({});
    expect(await refreshModelCatalog()).toEqual({
      status: "bundled",
      reason: "fetch_failed",
    });
  });

  it("keeps the bundled catalog when the hash sidecar is missing", async () => {
    const body = JSON.stringify(validRemoteCatalog());
    mockFetch({
      [REMOTE_MODEL_CATALOG_URL]: { text: body },
    });
    expect(await refreshModelCatalog()).toEqual({
      status: "bundled",
      reason: "hash_unavailable",
    });
  });

  it("keeps the bundled catalog when the sha256 does not match", async () => {
    const body = JSON.stringify(validRemoteCatalog());
    mockFetch({
      [REMOTE_MODEL_CATALOG_URL]: { text: body },
      [`${REMOTE_MODEL_CATALOG_URL}.sha256`]: {
        text: "deadbeef".repeat(8),
      },
    });
    expect(await refreshModelCatalog()).toEqual({
      status: "bundled",
      reason: "hash_mismatch",
    });
  });

  it("keeps the bundled catalog when the payload is malformed", async () => {
    const body = "not json at all";
    mockFetch({
      [REMOTE_MODEL_CATALOG_URL]: { text: body },
      [`${REMOTE_MODEL_CATALOG_URL}.sha256`]: { text: await sha256(body) },
    });
    expect(await refreshModelCatalog()).toEqual({
      status: "bundled",
      reason: "invalid_payload",
    });
  });

  it("keeps the bundled catalog when the revision is not newer", async () => {
    const body = JSON.stringify(
      validRemoteCatalog({ verifiedAt: catalog.verifiedAt }),
    );
    mockFetch({
      [REMOTE_MODEL_CATALOG_URL]: { text: body },
      [`${REMOTE_MODEL_CATALOG_URL}.sha256`]: { text: await sha256(body) },
    });
    expect(await refreshModelCatalog()).toEqual({
      status: "bundled",
      reason: "not_newer",
    });
  });
});
