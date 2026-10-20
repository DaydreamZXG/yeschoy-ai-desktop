import { afterEach, describe, expect, it } from "vitest";
import {
  clearCachedSignedInProjection,
  readCachedSignedInProjection,
  writeCachedSignedInProjection,
} from "./accountProjectionCache";
import type { AccountProjection } from "./session";

function signedIn(overrides: Partial<AccountProjection> = {}): AccountProjection {
  return {
    requestId: "cache-fixture",
    schemaVersion: 3,
    status: "signed_in",
    userCode: "",
    pollAfterSeconds: 0,
    expiresAtEpochMs: 0,
    observedAtEpochMs: Date.now(),
    account: {
      available: true,
      displayName: "张哥",
      username: "zhang",
      balanceQuota: "10",
      usedQuota: "1",
      requestCount: "2",
      quotaPerUnit: "500000",
    },
    usage: {
      available: false,
      consumedQuota: "",
      requestRate: "",
      tokenCount: "",
    },
    models: [],
    comparisonFx: "1",
    reasonCode: "signed_in",
    ...overrides,
  };
}

afterEach(() => {
  clearCachedSignedInProjection();
});

describe("account projection cache", () => {
  it("round-trips a signed-in snapshot for instant home paint", () => {
    writeCachedSignedInProjection(signedIn({ account: {
      available: true,
      displayName: "张哥",
      username: "zhang",
      balanceQuota: "42",
      usedQuota: "1",
      requestCount: "2",
      quotaPerUnit: "500000",
    }}));
    expect(readCachedSignedInProjection()?.account.displayName).toBe("张哥");
    expect(readCachedSignedInProjection()?.account.balanceQuota).toBe("42");
  });

  it("ignores non-signed-in and expired snapshots", () => {
    writeCachedSignedInProjection(signedIn({ status: "signed_out" as "signed_in" }));
    // status guard in writer: signed_out is not written
    expect(readCachedSignedInProjection()).toBeNull();
    writeCachedSignedInProjection(
      signedIn({ observedAtEpochMs: Date.now() - 40 * 24 * 60 * 60 * 1000 }),
    );
    expect(readCachedSignedInProjection()).toBeNull();
  });
});
