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
import type { AccountSessionController } from "../account/useAccountSession";

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
function signedInSession(): AccountSessionController {
  return {
    projection: {
      requestId: "account-test",
      schemaVersion: 3,
      status: "signed_in",
      userCode: "",
      pollAfterSeconds: 0,
      expiresAtEpochMs: 0,
      observedAtEpochMs: 1,
      account: {
        available: true,
        displayName: "Test",
        username: "test",
        balanceQuota: "1",
        usedQuota: "0",
        requestCount: "0",
        quotaPerUnit: "1",
      },
      usage: {
        available: true,
        consumedQuota: "0",
        requestRate: "0",
        tokenCount: "0",
      },
      models: [
        {
          id: "glm-5.3",
          description: "",
          billingMode: "ratio",
          pricingAvailable: true,
          officialInputCnyPerMillion: "2",
          officialOutputCnyPerMillion: "8",
          actualInputCnyPerMillion: "1",
          actualOutputCnyPerMillion: "4",
          supportedEndpointTypes: ["anthropic", "openai"],
          billing: {
            groups: [{ id: "default", description: "标准分组", ratio: 0.5 }],
            baseInputUsd: 2,
            baseOutputUsd: 8,
            requestUsd: null,
            expression: "",
          },
        },
      ],
      comparisonFx: "1",
      reasonCode: "none",
    },
    loading: false,
    refresh: vi.fn(async () => null),
    beginAuthorization: vi.fn(async () => null),
    cancelAuthorization: vi.fn(async () => null),
    logout: vi.fn(async () => null),
    openWallet: vi.fn(async () => true),
  };
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
  it("uses the shared signed-in model list without a second public catalog flow", async () => {
    const onLineChange = vi.fn();
    render(
      <ConfigurationPreviewView
        lineId="mainland_optimized"
        onLineChange={onLineChange}
        session={signedInSession()}
        onOpenAccount={vi.fn()}
        onOpenTools={vi.fn()}
      />,
    );
    const preview = screen.getByRole("region", { name: "完成接入" });
    expect(preview).toHaveTextContent("glm-5.3");
    expect(
      screen.getByRole("heading", { name: "选好，就能用" }),
    ).toBeInTheDocument();
    expect(screen.getByText("1 亿 Token 费用参考")).toBeInTheDocument();
    expect(screen.getByText("使用官网预计")).toBeInTheDocument();
    expect(screen.getByText("使用野菜预计")).toBeInTheDocument();
    expect(screen.getByText("¥260")).toBeInTheDocument();
    expect(screen.getByText("¥130")).toBeInTheDocument();
    expect(screen.getByText(/约省 50%/)).toBeInTheDocument();
    expect(
      screen.getByText("按你所在的位置选择，价格不会因此改变"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "一键接入",
    );
    fireEvent.click(
      screen.getByRole("button", { name: /大陆优化 中国大陆网络优先/ }),
    );
    expect(preview).toHaveTextContent("glm-5.3");
    expect(onLineChange).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: /全球加速 Cloudflare 全球线路/ }),
    );
    expect(onLineChange).toHaveBeenCalledWith("global_accelerated");
    expect(mockedInvoke).not.toHaveBeenCalled();
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
