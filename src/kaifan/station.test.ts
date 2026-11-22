import { beforeEach, describe, expect, it, vi } from "vitest";
import { probeStation } from "../station/stationProbe";
import { hostOfLine, lookAtStation, normalizeStationInput } from "./station";

vi.mock("../station/stationProbe", () => ({ probeStation: vi.fn() }));
const probe = vi.mocked(probeStation);
const result = (status: string) =>
  ({ status, desktopConnect: false }) as Awaited<
    ReturnType<typeof probeStation>
  >;

describe("用户贴进来的网址", () => {
  it.each([
    ["example.com", "https://example.com"],
    ["  https://Example.com/console/token?x=1 ", "https://example.com"],
    ["EXAMPLE.com/", "https://example.com"],
  ])("%s → %s", (raw, origin) => {
    expect(normalizeStationInput(raw)).toBe(origin);
  });

  it.each([
    "",
    "   ",
    "http://example.com",
    "not a url",
    "ftp://example.com",
    // 后端只认标准端口的 https 站点，带端口的这里就挡住。
    "example.com:8443",
  ])("%s 不收", (raw) => {
    expect(normalizeStationInput(raw)).toBeNull();
  });
});

describe("线路里带的站点地址", () => {
  it("自己填的站点能读出域名", () => {
    expect(hostOfLine("station_key:https://my.example.com")).toBe(
      "my.example.com",
    );
    expect(hostOfLine("station_connect:https://a.example.com")).toBe(
      "a.example.com",
    );
  });

  it("内置线路没有域名可说", () => {
    expect(hostOfLine("mainland_optimized")).toBeNull();
  });
});

describe("看一眼站点", () => {
  beforeEach(() => {
    probe.mockReset();
  });

  it("一时连不上：自己再试一次", async () => {
    probe
      .mockResolvedValueOnce(result("unreachable"))
      .mockResolvedValueOnce(result("ok"));
    expect((await lookAtStation("https://a.example.com")).ok).toBe(true);
    expect(probe).toHaveBeenCalledTimes(2);
  });

  it("两次都不行才报", async () => {
    probe.mockRejectedValue(new Error("boom"));
    expect(await lookAtStation("https://a.example.com")).toEqual({
      ok: false,
      reason: "unreachable",
    });
    expect(probe).toHaveBeenCalledTimes(2);
  });

  it("站点明确说不是中转站：不重试", async () => {
    probe.mockResolvedValue(result("not_relay"));
    expect(await lookAtStation("https://a.example.com")).toEqual({
      ok: false,
      reason: "not_relay",
    });
    expect(probe).toHaveBeenCalledTimes(1);
  });

  it("名单上不给连的站点：照实报，不重试", async () => {
    probe.mockResolvedValue(result("refused_station"));
    expect(await lookAtStation("https://api.open-design.ai")).toEqual({
      ok: false,
      reason: "refused_station",
    });
    expect(probe).toHaveBeenCalledTimes(1);
  });

  it("密钥原样交给探测", async () => {
    probe.mockResolvedValue(result("ok"));
    await lookAtStation("https://a.example.com", "sk-x");
    expect(probe).toHaveBeenCalledWith("https://a.example.com", "sk-x");
  });
});
