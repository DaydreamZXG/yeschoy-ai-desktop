import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import { ServiceCatalogPanel } from "./ServiceCatalogPanel";
import { ConfigurationPreviewView } from "../configuration/ConfigurationPreviewView";
import { catalogFixture } from "./test-fixtures";
import type { ConfigurationLineId } from "../configuration/preview";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockedInvoke = vi.mocked(invoke);
const callbacks = () => ({ onPlanChange: vi.fn(), onReadAttempt: vi.fn() });
type RequestArgs = {
  request: { requestId: string; lineId: ConfigurationLineId };
};
function respond() {
  mockedInvoke.mockImplementation(async (_command, args) => {
    const { request } = args as RequestArgs;
    return catalogFixture(request.requestId, request.lineId);
  });
}
beforeEach(async () => {
  mockedInvoke.mockReset();
  i18n.addResourceBundle(
    "zh",
    "translation",
    {
      yeschoyCatalog: zh.yeschoyCatalog,
      yeschoyConfiguration: zh.yeschoyConfiguration,
    },
    true,
    true,
  );
  await i18n.changeLanguage("zh");
});

describe("catalog selection and recovery", () => {
  it("preserves the shared preview when the selected line is clicked again", async () => {
    respond();
    render(
      <ConfigurationPreviewView
        onOpenAccount={vi.fn()}
        onOpenTools={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "获取模型列表" }));
    await screen.findByRole("combobox", { name: /服务分组/ });
    fireEvent.change(screen.getByRole("combobox", { name: /服务分组/ }), {
      target: { value: "test-group" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: /模型 ID/ }), {
      target: { value: "test/model" },
    });
    const preview = screen.getByRole("region", { name: "确认接入信息" });
    expect(preview).toHaveTextContent("test/model");
    fireEvent.click(
      screen.getByRole("button", { name: /大陆优化 中国大陆网络优先/ }),
    );
    expect(preview).toHaveTextContent("test/model");
    expect(preview).toHaveTextContent("test-group");
    expect(screen.getByRole("combobox", { name: /模型 ID/ })).toHaveValue(
      "test/model",
    );
    fireEvent.click(
      screen.getByRole("button", { name: /全球加速 Cloudflare 全球线路/ }),
    );
    expect(preview).not.toHaveTextContent("test/model");
    expect(preview).not.toHaveTextContent("test-group");
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    expect(mockedInvoke).toHaveBeenCalledTimes(1);
  });
  it("only reads on user action and emits a real ID preview without authorization", async () => {
    respond();
    const cb = callbacks();
    render(
      <ServiceCatalogPanel
        toolId="codex"
        lineId="mainland_optimized"
        {...cb}
      />,
    );
    expect(mockedInvoke).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "获取模型列表" }));
    await screen.findByRole("combobox", { name: /服务分组/ });
    expect(mockedInvoke).toHaveBeenCalledWith("read_public_service_catalog", {
      request: {
        requestId: expect.stringMatching(/^catalog-/),
        lineId: "mainland_optimized",
      },
    });
    fireEvent.change(screen.getByRole("combobox", { name: /服务分组/ }), {
      target: { value: "test-group" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: /模型 ID/ }), {
      target: { value: "test/model" },
    });
    await waitFor(() =>
      expect(cb.onPlanChange).toHaveBeenLastCalledWith(
        expect.objectContaining({
          modelId: "test/model",
          groupId: "test-group",
          applyAllowed: false,
        }),
      ),
    );
    expect(screen.getByText("登录功能暂未开放。")).toBeInTheDocument();
    expect(screen.getByText("已选模型，使用前仍需确认")).toBeInTheDocument();
  });
  it("clears selection on refresh and reports a valid empty catalog", async () => {
    respond();
    const cb = callbacks();
    render(
      <ServiceCatalogPanel toolId="pi" lineId="mainland_optimized" {...cb} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "获取模型列表" }));
    await screen.findByRole("combobox", { name: /服务分组/ });
    fireEvent.change(screen.getByRole("combobox", { name: /服务分组/ }), {
      target: { value: "test-group" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: /模型 ID/ }), {
      target: { value: "test/model" },
    });
    mockedInvoke.mockImplementation(async (_command, args) => ({
      ...catalogFixture((args as RequestArgs).request.requestId),
      groups: [],
      models: [],
    }));
    fireEvent.click(screen.getByRole("button", { name: "刷新列表" }));
    await screen.findByText(/暂无可选模型/);
    expect(cb.onPlanChange).toHaveBeenLastCalledWith(null);
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
  });
  it("discards a late response after the selected line changes", async () => {
    let finishOld: (value: unknown) => void = () => {};
    let previous: RequestArgs | undefined;
    mockedInvoke.mockImplementationOnce(
      (_command, args) =>
        new Promise((resolve) => {
          finishOld = resolve;
          previous = args as RequestArgs;
        }),
    );
    const cb = callbacks();
    const view = render(
      <ServiceCatalogPanel toolId="pi" lineId="mainland_optimized" {...cb} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "获取模型列表" }));
    expect(screen.getByRole("button", { name: "正在加载…" })).toBeDisabled();
    view.rerender(
      <ServiceCatalogPanel toolId="pi" lineId="global_accelerated" {...cb} />,
    );
    respond();
    fireEvent.click(screen.getByRole("button", { name: "获取模型列表" }));
    await screen.findByRole("combobox", { name: /服务分组/ });
    fireEvent.change(screen.getByRole("combobox", { name: /服务分组/ }), {
      target: { value: "test-group" },
    });
    const old = catalogFixture(previous!.request.requestId);
    old.models[0].id = "obsolete-model";
    await act(async () => finishOld(old));
    expect(
      screen.queryByRole("option", { name: "obsolete-model" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: "test/model" }),
    ).toBeInTheDocument();
  });
  it("rejects malformed data and hides raw errors while allowing recovery", async () => {
    mockedInvoke.mockRejectedValueOnce(
      "private-password <script>bad()</script>",
    );
    render(
      <ServiceCatalogPanel
        toolId="pi"
        lineId="mainland_optimized"
        {...callbacks()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "获取模型列表" }));
    await screen.findByRole("alert");
    expect(document.body.textContent).not.toContain("private-password");
    mockedInvoke.mockResolvedValueOnce({
      success: true,
      data: ["guessed-model"],
    });
    fireEvent.click(screen.getByRole("button", { name: "刷新列表" }));
    await screen.findByText(/模型列表加载失败/);
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    respond();
    fireEvent.click(screen.getByRole("button", { name: "刷新列表" }));
    await screen.findByRole("combobox", { name: /服务分组/ });
  });
  it("treats group prose as text rather than executable HTML or protocol evidence", async () => {
    mockedInvoke.mockImplementation(async (_command, args) => {
      const sample = catalogFixture((args as RequestArgs).request.requestId);
      sample.groups[0].description = '<img src="x" onerror="steal()">';
      sample.models[0].endpoints = [];
      return sample;
    });
    render(
      <ServiceCatalogPanel
        toolId="pi"
        lineId="mainland_optimized"
        {...callbacks()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "获取模型列表" }));
    await screen.findByRole("combobox", { name: /服务分组/ });
    fireEvent.change(screen.getByRole("combobox", { name: /服务分组/ }), {
      target: { value: "test-group" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: /模型 ID/ }), {
      target: { value: "test/model" },
    });
    expect(
      screen.getByText('<img src="x" onerror="steal()">'),
    ).toBeInTheDocument();
    expect(document.querySelector(".service-catalog-panel img")).toBeNull();
    expect(
      screen.getByText("暂不能确认此模型支持当前应用"),
    ).toBeInTheDocument();
  });
});
