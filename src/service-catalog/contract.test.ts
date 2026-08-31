import { describe, expect, it } from "vitest";
import { isServiceCatalog } from "./contract";
import { catalogFixture } from "./test-fixtures";

const valid = (value: unknown) =>
  isServiceCatalog(value, "test-read", "mainland_optimized");
describe("public catalog contract", () => {
  it("accepts a public catalog and a genuine empty catalog", () => {
    expect(valid(catalogFixture())).toBe(true);
    expect(valid({ ...catalogFixture(), groups: [], models: [] })).toBe(true);
  });
  it("rejects stale request identities and a different line", () => {
    expect(valid(catalogFixture("previous"))).toBe(false);
    expect(valid(catalogFixture("test-read", "global_accelerated"))).toBe(
      false,
    );
  });
  it("rejects malformed projections without inventing defaults", () => {
    for (const bad of [
      null,
      [],
      {},
      { ...catalogFixture(), catalogError: "unknown" },
      { ...catalogFixture(), catalogStatus: "ready" },
      { ...catalogFixture(), userSpecific: true },
      { ...catalogFixture(), secretsAccessed: true },
      { ...catalogFixture(), observedAtEpochMs: NaN },
      { ...catalogFixture(), observedAtEpochMs: Number.MAX_SAFE_INTEGER + 1 },
      { ...catalogFixture(), password: "must-not-exist" },
    ])
      expect(valid(bad)).toBe(false);
    for (const multiplier of [
      "",
      "0",
      "-1",
      "NaN",
      "Infinity",
      "0.25 extra",
      0.25,
    ])
      expect(
        valid({
          ...catalogFixture(),
          groups: [{ ...catalogFixture().groups[0], multiplier }],
        }),
      ).toBe(false);
  });
  it("rejects duplicate IDs, foreign groups, hidden fields and oversized values", () => {
    const sample = catalogFixture();
    expect(
      valid({ ...sample, groups: [...sample.groups, ...sample.groups] }),
    ).toBe(false);
    expect(
      valid({ ...sample, models: [...sample.models, ...sample.models] }),
    ).toBe(false);
    for (const change of [
      { groups: ["unknown"] },
      { groups: [] },
      { id: "a".repeat(201) },
      { id: "bad\u202e" },
      { endpoints: ["openai", "openai"] },
      { billingMode: "free" },
      { billing_expr: "must-not-cross-ipc" },
    ])
      expect(
        valid({ ...sample, models: [{ ...sample.models[0], ...change }] }),
      ).toBe(false);
    expect(
      valid({
        ...sample,
        models: Array.from({ length: 2049 }, (_, index) => ({
          ...sample.models[0],
          id: `model-${index}`,
        })),
      }),
    ).toBe(false);
  });
  it("keeps catalog and backend failures separate and consistent", () => {
    const sample = catalogFixture();
    expect(
      valid({
        ...sample,
        desktopBackend: {
          status: "unavailable",
          error: "timed_out",
          declaredCapabilities: [],
        },
      }),
    ).toBe(true);
    expect(
      valid({
        ...sample,
        catalogStatus: "unavailable",
        catalogError: "http_error",
        groups: [],
        models: [],
        serviceVersion: "",
        backendDisplayExchangeRate: "",
      }),
    ).toBe(true);
    expect(
      valid({
        ...sample,
        catalogStatus: "unavailable",
        catalogError: "http_error",
      }),
    ).toBe(false);
    expect(
      valid({
        ...sample,
        desktopBackend: {
          status: "not_deployed",
          error: "none",
          declaredCapabilities: ["account_read"],
        },
      }),
    ).toBe(false);
    expect(
      valid({
        ...sample,
        desktopBackend: {
          status: "contract_recognized",
          error: "none",
          declaredCapabilities: ["admin"],
        },
      }),
    ).toBe(false);
    expect(
      valid({
        ...sample,
        desktopBackend: {
          status: "contract_recognized",
          error: "none",
          declaredCapabilities: ["account_read"],
        },
      }),
    ).toBe(true);
  });
});
