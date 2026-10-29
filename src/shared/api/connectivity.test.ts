import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  CONNECTIVITY_LINES,
  CONNECTIVITY_LAYER_IDS,
  type ConnectivityResponse,
} from "../../diagnostics/contract";
import { checkLineConnectivity } from "./connectivity";
import { invokeDecoded } from "./invokeDecoded";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const native = vi.mocked(invoke);
function response(requestId: string): ConnectivityResponse {
  return {
    requestId,
    schemaVersion: 2,
    startedAtEpochMs: 1,
    completedAtEpochMs: 2,
    lines: CONNECTIVITY_LINES.map((line) => ({
      ...line,
      layers: [
        {
          layer: "dns",
          status: "passed",
          reasonCode: "dns_resolved",
          latencyMs: 1,
        },
        {
          layer: "tcp",
          status: "passed",
          reasonCode: "tcp443_reachable",
          latencyMs: 2,
        },
        {
          layer: "tls",
          status: "passed",
          reasonCode: "tls_handshake_verified",
          latencyMs: 3,
        },
        {
          layer: "api_key",
          status: "skipped",
          reasonCode: "skipped_no_saved_session",
        },
      ],
    })),
  };
}
function mockResponse(
  transform: (value: ConnectivityResponse) => unknown = (value) => value,
) {
  native.mockImplementation(async (_command, args) => {
    const request = (args as { request: { requestId: string } }).request;
    return transform(response(request.requestId));
  });
}
beforeEach(() => {
  native.mockReset();
});

describe("decoded native API boundary", () => {
  it("invokes only the existing read-only command and returns validated evidence", async () => {
    mockResponse();
    const decoded = await checkLineConnectivity();
    expect(decoded.complete).toBe(true);
    expect(decoded.response.lines).toHaveLength(2);
    expect(native).toHaveBeenCalledWith("check_line_connectivity_read_only", {
      request: { requestId: decoded.response.requestId },
    });
    expect(decoded.response.requestId).toMatch(/^[A-Za-z0-9_-]{1,64}$/);
  });

  it.each([
    [
      "unexpected top-level field",
      (value: ConnectivityResponse) => ({
        ...value,
        leakedPath: "fixture-only",
      }),
    ],
    [
      "mismatched correlation ID",
      (value: ConnectivityResponse) => ({
        ...value,
        requestId: "another-request",
      }),
    ],
    [
      "unknown schema",
      (value: ConnectivityResponse) => ({ ...value, schemaVersion: 99 }),
    ],
  ])("fails closed on %s", async (_name, transform) => {
    mockResponse(transform);
    await expect(checkLineConnectivity()).rejects.toThrow(
      "invalid_connectivity_response",
    );
  });

  it("rejects malformed nested evidence without losing independently validated lines", async () => {
    mockResponse((value) => ({
      ...value,
      lines: [
        {
          ...value.lines[0],
          layers: value.lines[0].layers.map((layer) => ({
            ...layer,
            unknown: true,
          })),
        },
        value.lines[1],
      ],
    }));
    const decoded = await checkLineConnectivity();
    expect(decoded.complete).toBe(false);
    expect(decoded.response.lines.map((line) => line.lineId)).toEqual([
      "global_accelerated",
    ]);
  });

  it("passes native rejection to the hook without fabricating a network result", async () => {
    native.mockRejectedValueOnce(new Error("fixture_native_failure"));
    await expect(checkLineConnectivity()).rejects.toThrow(
      "fixture_native_failure",
    );
  });

  it("never returns an undecoded value from the generic boundary", async () => {
    native.mockResolvedValueOnce({ leakedPath: "fixture-only" });
    await expect(
      invokeDecoded(
        "fixture_read_only",
        {},
        () => null,
        "invalid_fixture_response",
      ),
    ).rejects.toThrow("invalid_fixture_response");
  });
});

describe("read-only connectivity cross-language contract", () => {
  it("keeps request/response names and camelCase policy aligned with Rust declarations", () => {
    const source = readFileSync(
      resolve(process.cwd(), "src-tauri/src/connectivity.rs"),
      "utf8",
    );
    const fields = (name: string) => {
      const body = source.match(
        new RegExp(`pub struct ${name} \\{([\\s\\S]*?)\\n\\}`),
      )?.[1];
      expect(body).toBeDefined();
      return [...body!.matchAll(/^\s+(\w+):/gm)].map((match) =>
        match[1].replace(/_([a-z])/g, (_all, letter: string) =>
          letter.toUpperCase(),
        ),
      );
    };
    expect(source).toMatch(
      /#\[serde\(rename_all = "camelCase", deny_unknown_fields\)\]\s*pub struct ConnectivityRequest/,
    );
    expect(fields("ConnectivityRequest")).toEqual(["requestId"]);
    expect(fields("ConnectivityResponse")).toEqual(
      Object.keys(response("fixture-contract")),
    );
  });

  it("keeps the layer order and fixed line metadata aligned with Rust", () => {
    const source = readFileSync(
      resolve(process.cwd(), "src-tauri/src/connectivity_core.rs"),
      "utf8",
    );
    const layerList = source.match(
      /pub const CONNECTIVITY_LAYER_IDS[^=]+=[\s]*\[([^\]]+)\]/,
    )?.[1];
    expect(layerList).toBeDefined();
    expect(
      [...layerList!.matchAll(/"([^"]+)"/g)].map((match) => match[1]),
    ).toEqual(CONNECTIVITY_LAYER_IDS);
    const lineList = source.match(
      /pub const CONNECTIVITY_LINES[^=]+=[\s]*\[([\s\S]*?)\n\];/,
    )?.[1];
    expect(lineList).toBeDefined();
    const nativeLines = [...lineList!.matchAll(/LineSpec \{([\s\S]*?)\}/g)].map(
      (match) => {
        const text = (name: string) =>
          match[1].match(new RegExp(`${name}: "([^"]+)"`))?.[1];
        return {
          lineId: text("line_id"),
          displayName: text("display_name"),
          rootUrl: text("root_url"),
          host: text("host"),
          port: Number(match[1].match(/port: (\d+)/)?.[1]),
        };
      },
    );
    expect(nativeLines).toEqual(CONNECTIVITY_LINES);
  });
});
