import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AccountModel, AccountProjection } from "../../account/session";
import type { AccountSessionController } from "../../account/useAccountSession";
import { connectionsFixture } from "../../configuration/connection-test-fixtures";
import { useActivationSelection } from "./useActivationSelection";

vi.mock("sonner", () => ({ toast: vi.fn() }));
function model(id: string): AccountModel {
  return {
    id,
    description: "",
    billingMode: "ratio",
    pricingAvailable: false,
    officialInputCnyPerMillion: "",
    officialOutputCnyPerMillion: "",
    actualInputCnyPerMillion: "",
    actualOutputCnyPerMillion: "",
    billing: {
      groups: [
        { id: "default", description: "", ratio: 1 },
        { id: "fixture-discount", description: "", ratio: 0.5 },
      ],
      baseInputUsd: 1,
      baseOutputUsd: 2,
      cacheReadUsd: null,
      requestUsd: null,
      expression: "",
    },
  };
}
function inputs(
  ids = ["fixture-a", "fixture-b"],
): Parameters<typeof useActivationSelection>[0] {
  const projection: AccountProjection = {
    requestId: "fixture-account",
    schemaVersion: 3,
    status: "signed_in",
    userCode: "",
    pollAfterSeconds: 0,
    expiresAtEpochMs: 0,
    observedAtEpochMs: 1,
    reasonCode: "none",
    comparisonFx: "",
    account: {
      available: true,
      displayName: "fixture",
      username: "fixture-user",
      balanceQuota: "0",
      usedQuota: "0",
      requestCount: "0",
      quotaPerUnit: "500000",
    },
    usage: {
      available: false,
      consumedQuota: "",
      requestRate: "",
      tokenCount: "",
    },
    models: ids.map(model),
  };
  const session: AccountSessionController = {
    projection,
    loading: false,
    refresh: vi.fn(async () => projection),
    beginAuthorization: vi.fn(async () => null),
    openAuthorization: vi.fn(async () => null),
    cancelAuthorization: vi.fn(async () => null),
    logout: vi.fn(async () => null),
    openWallet: vi.fn(async () => true),
  };
  return {
    session,
    activationToolId: "codex_desktop",
    applyPhase: "idle",
    connectionStateUnavailable: false,
    lineId: "mainland_optimized",
    onLineChange: vi.fn(),
    lineFollowedToast: "fixture-line-followed",
    onReset: vi.fn(),
  };
}
afterEach(cleanup);

describe("editable activation selection", () => {
  it("restores the exact saved price and missing model IDs without substituting a new choice", () => {
    const input = inputs();
    input.savedConnection = {
      ...connectionsFixture("fixture").connections[0],
      toolId: "codex_desktop",
      state: "connected",
      modelId: "fixture-retired",
      billingGroup: "fixture-retired-group",
      lineId: "global_accelerated",
    };
    const { result } = renderHook(() => useActivationSelection(input));
    expect(result.current.selectedModelId).toBe("fixture-retired");
    expect(result.current.billingGroup).toBe("fixture-retired-group");
    expect(result.current.selectedModel).toBeUndefined();
    expect(input.onLineChange).toHaveBeenCalledWith("global_accelerated");
  });

  it("adding a model edits its price without changing the default", () => {
    const input = inputs();
    const { result } = renderHook(() => useActivationSelection(input));
    act(() => result.current.startAddingModel());
    act(() => result.current.chooseModel("fixture-b"));
    act(() => result.current.updatePricedGroup("fixture-discount"));
    expect(result.current.defaultBinding?.modelId).toBe("fixture-a");
    expect(result.current.priceModelId).toBe("fixture-b");
    expect(result.current.submittedModels).toEqual([
      { modelId: "fixture-a", billingGroup: "default" },
      { modelId: "fixture-b", billingGroup: "fixture-discount" },
    ]);
  });

  it("removing the default requires an explicit replacement and does not resubmit automatically", () => {
    const input = inputs();
    const { result } = renderHook(() => useActivationSelection(input));
    act(() => result.current.startAddingModel());
    act(() => result.current.chooseModel("fixture-b"));
    act(() => result.current.removeBinding("fixture-a"));
    expect(result.current.selectedModelId).toBe("");
    expect(result.current.defaultBinding).toBeUndefined();
    expect(result.current.defaultMissing).toBe(true);
    expect(
      result.current.submittedModels.map((binding) => binding.modelId),
    ).toEqual(["fixture-b"]);
    expect(input.onReset).toHaveBeenCalledTimes(2);
  });

  it("does not reset user edits on a same-account refresh", () => {
    const input = inputs();
    const { result, rerender } = renderHook(useActivationSelection, {
      initialProps: input,
    });
    act(() => result.current.chooseModel("fixture-b"));
    act(() => result.current.updatePricedGroup("fixture-discount"));
    rerender({
      ...input,
      session: {
        ...input.session,
        projection: {
          ...input.session.projection!,
          models: [model("fixture-a"), model("fixture-b")],
        },
      },
    });
    expect(result.current.selectedModelId).toBe("fixture-b");
    expect(result.current.billingGroup).toBe("fixture-discount");
  });

  it("updates render-time readiness for a changed account with identical restored values", () => {
    const input = inputs();
    const { result, rerender } = renderHook(useActivationSelection, {
      initialProps: input,
    });
    const next = {
      ...input,
      session: {
        ...input.session,
        projection: {
          ...input.session.projection!,
          account: {
            ...input.session.projection!.account,
            username: "fixture-next-user",
          },
        },
      },
    };
    rerender(next);
    expect(result.current.selectionReadyKey).toBe(result.current.selectionKey);
    expect(result.current.selectionReadyKey).toBe(
      "fixture-next-user:codex_desktop",
    );
  });

  it("keeps choice initialization suspended while a native activation is applying", () => {
    const input = { ...inputs(), applyPhase: "applying" as const };
    const { result } = renderHook(() => useActivationSelection(input));
    expect(result.current.selectionReadyKey).toBeNull();
    expect(result.current.selectedModelId).toBe("");
  });

  it("does not automatically enroll a 201st model or change the previous default", () => {
    const ids = Array.from(
      { length: 201 },
      (_value, index) => `fixture-model-${index}`,
    );
    const input = inputs(ids);
    input.savedConnection = {
      ...connectionsFixture("fixture").connections[0],
      toolId: "codex_desktop",
      state: "connected",
      modelId: ids[0],
      billingGroup: "default",
      models: ids
        .slice(0, 200)
        .map((modelId) => ({ modelId, billingGroup: "default" })),
    };
    const { result } = renderHook(() => useActivationSelection(input));
    act(() => result.current.chooseModel(ids[200]));
    expect(result.current.pendingModelEdit).toBe(true);
    expect(result.current.defaultBinding?.modelId).toBe(ids[0]);
    expect(result.current.submittedModels).toHaveLength(200);
    expect(
      result.current.submittedModels.some(
        (binding) => binding.modelId === ids[200],
      ),
    ).toBe(false);
  });
});
