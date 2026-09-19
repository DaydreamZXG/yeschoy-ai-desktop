import { afterEach, describe, expect, it, vi } from "vitest";
import catalog from "./catalog.json";
import { applyModelCatalogOverride } from "./profile";
import { invoke } from "@tauri-apps/api/core";
import {
  parseRemoteCatalog,
  refreshModelCatalog,
  type BundledCatalogModel,
} from "./remoteCatalog";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const native = vi.mocked(invoke);

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

/** The native reply shape of `refresh_model_catalog_v1`. */
function nativeReply(over: Record<string, unknown> = {}) {
  return {
    requestId: "catalog-test",
    schemaVersion: 1,
    status: "updated",
    reasonCode: "updated",
    verifiedAt: "2099-01-01",
    modelCount: 1,
    catalog: validRemoteCatalog(),
    ...over,
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
  native.mockReset();
  applyModelCatalogOverride(null);
});

describe("parseRemoteCatalog validates untrusted payloads", () => {
  it("accepts a well-formed catalog", () => {
    expect(parseRemoteCatalog(validRemoteCatalog())).not.toBeNull();
  });

  it("rejects unknown schema versions, bad dates, and empty models", () => {
    expect(
      parseRemoteCatalog(validRemoteCatalog({ schemaVersion: 2 })),
    ).toBeNull();
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

describe("refreshModelCatalog adopts what the native side adopted", () => {
  it("applies a newer, verified revision and makes its capabilities visible", async () => {
    native.mockResolvedValue(nativeReply());
    expect(await refreshModelCatalog()).toEqual({
      status: "updated",
      verifiedAt: "2099-01-01",
      modelCount: 1,
    });
    expect(native).toHaveBeenCalledWith(
      "refresh_model_catalog_v1",
      expect.objectContaining({ requestId: expect.any(String) }),
    );
    expect(
      (await import("./profile")).modelCapabilities("future-model"),
    ).toMatchObject({ contextWindow: 1000000 });
  });

  // The native side decides each of these; this asserts the renderer reports
  // the cause it was given rather than flattening them all into one fallback.
  it.each([
    "fetch_failed",
    "hash_unavailable",
    "hash_mismatch",
    "invalid_payload",
    "not_newer",
  ] as const)("reports %s as the native side classified it", async (reason) => {
    native.mockResolvedValue(
      nativeReply({ status: "bundled", reasonCode: reason, catalog: null }),
    );
    expect(await refreshModelCatalog()).toEqual({
      status: "bundled",
      reason,
    });
  });

  it("keeps the bundled catalog when the IPC call itself fails", async () => {
    native.mockRejectedValue(new Error("unavailable"));
    expect(await refreshModelCatalog()).toEqual({
      status: "bundled",
      reason: "fetch_failed",
    });
  });

  it("refuses a reply that claims success but carries no usable catalog", async () => {
    native.mockResolvedValue(nativeReply({ catalog: { nonsense: true } }));
    expect(await refreshModelCatalog()).toEqual({
      status: "bundled",
      reason: "invalid_payload",
    });
  });

  // Defence in depth: the native side already refuses a revision that is not
  // newer, but this side must not adopt one either if that check ever slips.
  it("refuses a payload that is not newer even when the reply says updated", async () => {
    native.mockResolvedValue(
      nativeReply({
        catalog: validRemoteCatalog({ verifiedAt: catalog.verifiedAt }),
      }),
    );
    expect(await refreshModelCatalog()).toEqual({
      status: "bundled",
      reason: "invalid_payload",
    });
  });

  it("refuses an unrecognised reply shape", async () => {
    native.mockResolvedValue({ schemaVersion: 99, status: "updated" });
    expect(await refreshModelCatalog()).toEqual({
      status: "bundled",
      reason: "invalid_payload",
    });
  });
});
