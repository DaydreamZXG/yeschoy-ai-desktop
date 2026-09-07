import { describe, expect, it } from "vitest";
import nativeFixtures from "../../tests/work_packages/ru041/fixtures/connectivity-native.json";
import {
  CONNECTIVITY_LINES,
  decodeConnectivityProjection,
  isCompleteConnectivityProjection,
} from "./contract";

function response() {
  return structuredClone(nativeFixtures.reachable);
}

describe("line connectivity projection from native serialization", () => {
  it.each(Object.entries(nativeFixtures))(
    "accepts the Rust-validated %s fixture",
    (_name, fixture) => {
      expect(isCompleteConnectivityProjection(fixture, fixture.requestId)).toBe(
        true,
      );
      expect(decodeConnectivityProjection(fixture, fixture.requestId)).toEqual({
        response: fixture,
        complete: true,
      });
    },
  );

  it.each([undefined, null, false, 0, "invalid", [], {}, { lines: [] }])(
    "handles an unknown malformed reply without throwing: %j",
    (value) => {
      expect(decodeConnectivityProjection(value, "diag-1")).toBeNull();
      expect(isCompleteConnectivityProjection(value, "diag-1")).toBe(false);
    },
  );

  it("rejects stale IDs, unknown envelope fields and invalid clocks", () => {
    const fixture = response();
    expect(decodeConnectivityProjection(fixture, "diag-new")).toBeNull();
    for (const update of [
      { requestId: "" },
      { accessToken: "synthetic-private" },
      { startedAtEpochMs: -1 },
      { startedAtEpochMs: 1.5 },
      { completedAtEpochMs: fixture.startedAtEpochMs - 1 },
      { completedAtEpochMs: Number.NaN },
      { completedAtEpochMs: Number.POSITIVE_INFINITY },
      { completedAtEpochMs: 8_640_000_000_000_001 },
      { lines: null },
      { lines: {} },
    ]) {
      expect(
        decodeConnectivityProjection(
          { ...fixture, ...update },
          fixture.requestId,
        ),
      ).toBeNull();
    }
    for (const requestId of [
      "",
      "../result",
      "https://example.com",
      "x".repeat(65),
    ]) {
      expect(
        decodeConnectivityProjection({ ...fixture, requestId }, requestId),
      ).toBeNull();
    }
  });

  it("isolates the old tcp443 spelling while retaining the other valid line", () => {
    const fixture = response();
    fixture.lines[0].reasonCode = "tcp443_reachable";
    const decoded = decodeConnectivityProjection(fixture, fixture.requestId);
    expect(decoded?.complete).toBe(false);
    expect(decoded?.response.lines).toEqual([
      nativeFixtures.reachable.lines[1],
    ]);
    expect(isCompleteConnectivityProjection(fixture, fixture.requestId)).toBe(
      false,
    );
  });

  it("rejects invalid identity, extra fields, unknown outcomes and impossible latency per line", () => {
    const fixture = response();
    for (const update of [
      { displayName: "other" },
      { rootUrl: "https://example.com" },
      { host: "example.com" },
      { port: 80 },
      { latencyMs: -1 },
      { latencyMs: 60_001 },
      { latencyMs: 1.5 },
      { latencyMs: Number.NaN },
      { status: "unknown", reasonCode: undefined },
      { status: "constructor", reasonCode: undefined },
      { reasonCode: "tcp_connection_failed" },
      { resolvedAddress: "127.0.0.1" },
      { error: "synthetic-private" },
    ]) {
      const payload = {
        ...fixture,
        lines: [{ ...fixture.lines[0], ...update }, fixture.lines[1]],
      };
      const decoded = decodeConnectivityProjection(payload, fixture.requestId);
      expect(decoded?.complete).toBe(false);
      expect(decoded?.response.lines).toEqual([fixture.lines[1]]);
      expect(JSON.stringify(decoded)).not.toContain("synthetic-private");
    }
  });

  it("keeps a line without turning a missing or malformed peer into a network failure", () => {
    const fixture = response();
    for (const lines of [
      [fixture.lines[0]],
      [fixture.lines[0], null],
      [fixture.lines[0], "invalid"],
    ]) {
      expect(
        decodeConnectivityProjection({ ...fixture, lines }, fixture.requestId),
      ).toEqual({
        response: { ...fixture, lines: [fixture.lines[0]] },
        complete: false,
      });
    }
    expect(
      decodeConnectivityProjection(
        { ...fixture, lines: [] },
        fixture.requestId,
      ),
    ).toEqual({
      response: { ...fixture, lines: [] },
      complete: false,
    });
  });

  it("does not choose between duplicate observations or lose the independent line", () => {
    const fixture = response();
    const payload = {
      ...fixture,
      lines: [fixture.lines[0], fixture.lines[0], fixture.lines[1]],
    };
    const decoded = decodeConnectivityProjection(payload, fixture.requestId);
    expect(decoded?.complete).toBe(false);
    expect(decoded?.response.lines).toEqual([fixture.lines[1]]);
    expect(isCompleteConnectivityProjection(payload, fixture.requestId)).toBe(
      false,
    );
  });

  it("only emits compiled destinations when an unrecognized third line is present", () => {
    const fixture = response();
    const payload = {
      ...fixture,
      lines: [...fixture.lines, { lineId: "unexpected", host: "example.com" }],
    };
    const decoded = decodeConnectivityProjection(payload, fixture.requestId);
    expect(decoded?.complete).toBe(false);
    expect(decoded?.response.lines).toEqual(fixture.lines);
    expect(
      decoded?.response.lines.map(({ lineId, host, port }) => ({
        lineId,
        host,
        port,
      })),
    ).toEqual(
      CONNECTIVITY_LINES.map(({ lineId, host, port }) => ({
        lineId,
        host,
        port,
      })),
    );
    expect(JSON.stringify(decoded)).not.toContain("example.com");
  });
});
