import { describe, expect, it } from "vitest";
import nativeFixtures from "../../tests/work_packages/ru041/fixtures/connectivity-native.json";
import {
  CONNECTIVITY_LINES,
  decodeConnectivityProjection,
  isCompleteConnectivityProjection,
  lineNetworkHealthy,
  lineOutcome,
} from "./contract";
import type { ConnectivityLineResult } from "./contract";

// JSON module types stay wide (string fields); narrow at the assertions.
const asLine = (value: unknown): ConnectivityLineResult =>
  value as ConnectivityLineResult;

function response() {
  return structuredClone(nativeFixtures.reachable);
}

describe("layered connectivity projection from native serialization", () => {
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

  it("rejects stale IDs, unknown envelope fields, wrong schema and invalid clocks", () => {
    const fixture = response();
    expect(decodeConnectivityProjection(fixture, "diag-new")).toBeNull();
    for (const update of [
      { requestId: "" },
      { accessToken: "synthetic-private" },
      { schemaVersion: 1 },
      { schemaVersion: 3 },
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

  it("rejects invalid line identity and injected fields per line", () => {
    const fixture = response();
    for (const update of [
      { displayName: "other" },
      { rootUrl: "https://example.com" },
      { host: "example.com" },
      { port: 80 },
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

  it("rejects a line whose layer catalog is reordered, incomplete or extended", () => {
    const fixture = response();
    const line = fixture.lines[0];
    for (const mutate of [
      (layers: unknown[]) => [layers[1], layers[0], layers[2], layers[3]],
      (layers: unknown[]) => layers.slice(0, 3),
      (layers: unknown[]) => [...layers, layers[3]],
      (layers: unknown[]) => layers.map(() => ({})),
    ]) {
      const payload = {
        ...fixture,
        lines: [
          { ...line, layers: mutate(line.layers) },
          fixture.lines[1],
        ],
      };
      const decoded = decodeConnectivityProjection(payload, fixture.requestId);
      expect(decoded?.complete).toBe(false);
      expect(decoded?.response.lines).toEqual([fixture.lines[1]]);
    }
  });

  it("rejects layer evidence whose status, reason or latency contradicts the contract", () => {
    const fixture = response();
    const line = fixture.lines[0];
    for (const mutate of [
      // status inconsistent with the reason code
      (layer: Record<string, unknown>) => ({ ...layer, status: "skipped" }),
      (layer: Record<string, unknown>) => ({ ...layer, status: "failed" }),
      // reason code from a different layer
      (layer: Record<string, unknown>) => ({
        ...layer,
        reasonCode: "tls_handshake_verified",
      }),
      // unknown reason code
      (layer: Record<string, unknown>) => ({
        ...layer,
        reasonCode: "tcp_443_reachable",
      }),
      // injected secret-looking field
      (layer: Record<string, unknown>) => ({
        ...layer,
        accessToken: "synthetic-private",
      }),
    ]) {
      const layers = line.layers.map((layer, index) =>
        index === 0 ? mutate({ ...layer }) : layer,
      );
      const payload = {
        ...fixture,
        lines: [{ ...line, layers }, fixture.lines[1]],
      };
      const decoded = decodeConnectivityProjection(payload, fixture.requestId);
      expect(decoded?.complete).toBe(false);
      expect(decoded?.response.lines).toEqual([fixture.lines[1]]);
      expect(JSON.stringify(decoded)).not.toContain("synthetic-private");
    }
    // A skipped layer must not carry latency (reachable fixture has none
    // skipped, so start from the mixed fixture).
    const mixed = structuredClone(nativeFixtures.mixed);
    const skipped = mixed.lines[1].layers.find(
      (layer) => layer.status === "skipped",
    ) as Record<string, unknown>;
    skipped.latencyMs = 5;
    const decoded = decodeConnectivityProjection(mixed, mixed.requestId);
    expect(decoded?.complete).toBe(false);
    expect(decoded?.response.lines).toEqual([mixed.lines[0]]);
  });

  it("rejects impossible layer latency values", () => {
    const fixture = response();
    const line = fixture.lines[0];
    for (const latencyMs of [-1, 60_001, 1.5, Number.NaN]) {
      const layers = line.layers.map((layer, index) =>
        index === 0 ? { ...layer, latencyMs } : layer,
      );
      const payload = {
        ...fixture,
        lines: [{ ...line, layers }, fixture.lines[1]],
      };
      const decoded = decodeConnectivityProjection(payload, fixture.requestId);
      expect(decoded?.complete).toBe(false);
      expect(decoded?.response.lines).toEqual([fixture.lines[1]]);
    }
  });

  it("derives the headline outcome from the first failed layer", () => {
    const reachable = asLine(nativeFixtures.reachable.lines[0]);
    expect(lineOutcome(reachable)).toBe("reachable");
    expect(lineNetworkHealthy(reachable)).toBe(true);

    const dnsFailure = asLine(nativeFixtures.mixed.lines[1]);
    expect(lineOutcome(dnsFailure)).toBe("dns_failed");
    expect(lineNetworkHealthy(dnsFailure)).toBe(false);

    const tcpFailure = asLine(nativeFixtures.failed.lines[0]);
    expect(lineOutcome(tcpFailure)).toBe("connect_failed");

    const dnsTimeout = asLine(nativeFixtures.failed.lines[1]);
    expect(lineOutcome(dnsTimeout)).toBe("timed_out");
  });

  it("keeps api_key failures out of the network reachability verdict", () => {
    const line = structuredClone(nativeFixtures.reachable.lines[0]);
    const layers = line.layers.map((layer) =>
      layer.layer === "api_key"
        ? { ...layer, status: "failed", reasonCode: "session_token_rejected" }
        : layer,
    );
    const rejected = { ...line, layers };
    expect(lineOutcome(asLine(rejected))).toBe("api_key_failed");
    expect(lineNetworkHealthy(asLine(rejected))).toBe(true);
  });
});
