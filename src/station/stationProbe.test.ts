import { describe, expect, it } from "vitest";
import { decodeProbe } from "./stationProbe";

describe("decodeProbe", () => {
  const ok = {
    origin: "https://r.example",
    status: "ok",
    kind: "new_api",
    systemName: "x",
    version: "v1",
    desktopConnect: false,
    tokenStatus: "valid",
    models: ["a"],
  };
  it("接受合法结果", () => {
    expect(decodeProbe(ok)?.models).toEqual(["a"]);
  });
  it("拒绝未知状态和坏字段", () => {
    expect(decodeProbe({ ...ok, status: "weird" })).toBeNull();
    expect(decodeProbe({ ...ok, models: [1] })).toBeNull();
    expect(decodeProbe(null)).toBeNull();
  });
});
