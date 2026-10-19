import { describe, expect, it } from "vitest";
import { decodeProbe } from "./stationProbe";
import { normalizeStationInput } from "../workbench/StationConnectOriginForm";

describe("小白粘贴的站点网址", () => {
  it("补 https、去掉路径、统一小写", () => {
    expect(normalizeStationInput("Relay.Example.com/console/token")).toBe(
      "https://relay.example.com",
    );
    expect(normalizeStationInput(" https://relay.example.com/ ")).toBe(
      "https://relay.example.com",
    );
  });
  it("拒绝 http、IP 和空输入", () => {
    expect(normalizeStationInput("http://relay.example.com")).toBeNull();
    expect(normalizeStationInput("https://1.2.3.4")).toBeNull();
    expect(normalizeStationInput("   ")).toBeNull();
  });
});

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
