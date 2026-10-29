import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import type { AccountSessionController } from "../../account/useAccountSession";
import type { ToolConnection } from "../../configuration/connections";
import type {
  ActivationToolId,
  ModelBinding,
} from "../../configuration/activation";
import type {
  AccountLineId,
  ConfigurationLineId,
} from "../../configuration/preview";
import { chooseBillingGroup } from "../../configuration/billing";
import {
  hasReviewedCapabilities,
  isImageGenerationModel,
} from "../../model-profiles/profile";
import { CATALOG_MENU_TOOLS } from "./previewRules";

/** Owns the editable choice, independently of the captured native task context. */
export function useActivationSelection({
  session,
  activationToolId,
  applyPhase,
  savedConnection,
  connectionStateUnavailable,
  lineId,
  onLineChange,
  lineFollowedToast,
  onReset,
}: {
  session: AccountSessionController;
  activationToolId: ActivationToolId;
  applyPhase: "idle" | "applying" | "finished";
  savedConnection?: ToolConnection;
  connectionStateUnavailable: boolean;
  lineId: AccountLineId;
  onLineChange: (lineId: ConfigurationLineId) => void;
  lineFollowedToast: string;
  onReset: () => void;
}) {
  const signedIn = session.projection?.status === "signed_in";
  const [selectedModelId, setSelectedModelId] = useState("");
  const [billingGroup, setBillingGroup] = useState("");
  const [modelSet, setModelSet] = useState<ModelBinding[]>([]);
  const [defaultModelId, setDefaultModelId] = useState("");
  // 再加一个打开的是上面那个模型浮层，不是另一套选择器。为 true 时，
  // 在浮层里点中的模型加进名单，不替换默认。
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const [pickerAdds, setPickerAdds] = useState(false);
  // 价格卡片正在编辑的模型。空着就跟着下拉框里的默认模型。
  const [focusedModelId, setFocusedModelId] = useState<string | null>(null);
  // This value participates in render-time readiness. Keeping it in a ref can
  // strand the action button when an account changes but the restored model
  // and group happen to be identical: the ref changes, all state setters bail
  // out as no-ops, and React has no reason to render the now-ready state.
  const [selectionReadyKey, setSelectionReadyKey] = useState<string | null>(
    null,
  );
  const accountModels =
    signedIn && session.projection
      ? session.projection.models.filter(
          (model) => !isImageGenerationModel(model.id),
        )
      : [];
  // 不再按协议筛选。每个模型都能选，理由见 modelCompatibility.ts 顶部。
  const models = accountModels;
  const selectedModel = models.find((model) => model.id === selectedModelId);
  const selectedBillingGroup = selectedModel?.billing?.groups.find(
    (g) => g.id === billingGroup,
  );
  const selectionKey = `${session.projection?.account.username ?? ""}:${activationToolId}`;
  useEffect(() => {
    if (
      selectionReadyKey === selectionKey ||
      !signedIn ||
      session.loading ||
      session.lastError ||
      applyPhase === "applying" ||
      connectionStateUnavailable
    )
      return;
    if (!models.length && !savedConnection?.modelId) return;
    setSelectionReadyKey(selectionKey);
    const existing =
      savedConnection && savedConnection.state !== "not_connected";
    // Retain missing IDs as visible, unselected choices. Never substitute a
    // different price for a saved choice, including after a transient refresh.
    // 默认项优先落在目录里有资料的模型上，见 `hasReviewedCapabilities`。
    // 一个已保存的连接永远说了算，这里只影响第一次配置。
    const fallback =
      models.find((model) => hasReviewedCapabilities(model.id)) ?? models[0];
    setSelectedModelId(
      existing ? savedConnection.modelId : (fallback?.id ?? ""),
    );
    setBillingGroup(
      existing
        ? savedConnection.billingGroup
        : chooseBillingGroup(fallback, ""),
    );
    setModelSet(existing ? (savedConnection.models ?? []) : []);
    setDefaultModelId(existing ? savedConnection.modelId : "");
    setFocusedModelId(null);
    setPickerAdds(false);
    setModelPickerOpen(false);
    if (existing && savedConnection.lineId) {
      // The global line picker visibly follows the app's last-used line;
      // announce the linkage instead of changing it silently (PRD 6.2).
      if (savedConnection.lineId !== lineId) toast(lineFollowedToast);
      onLineChange(savedConnection.lineId);
    }
  }, [
    selectionKey,
    selectionReadyKey,
    signedIn,
    session.loading,
    session.lastError,
    applyPhase,
    models,
    savedConnection,
    connectionStateUnavailable,
    lineId,
    lineFollowedToast,
    onLineChange,
  ]);

  const writesModelMenu = CATALOG_MENU_TOOLS.has(activationToolId);
  const dropdownBinding: ModelBinding | undefined =
    selectedModelId && billingGroup
      ? { modelId: selectedModelId, billingGroup }
      : undefined;
  const dropdownEnrolled = modelSet.some((m) => m.modelId === selectedModelId);
  // 下拉框里的模型自动进菜单，不需要再点一次「加入」。已经在名单里就沿用
  // 它的位置，只把计费分组换成当前选的；不在名单里就补到末尾。满 200 个时
  // 不再把一个新的塞进去。
  const menuAtCap =
    writesModelMenu &&
    !!dropdownBinding &&
    !dropdownEnrolled &&
    modelSet.length >= 200;
  const submittedModels: ModelBinding[] = writesModelMenu
    ? dropdownBinding && !menuAtCap
      ? dropdownEnrolled
        ? modelSet.map((m) =>
            m.modelId === dropdownBinding.modelId ? dropdownBinding : m,
          )
        : [...modelSet, dropdownBinding]
      : modelSet
    : dropdownBinding
      ? [dropdownBinding]
      : [];
  // 默认就是下拉框里这一个，写进 config 的 `model`。默认被移走、选择器也清空
  // 之后，不拿名单里的下一个顶上 —— 那会替用户改计费对象。
  const defaultBinding =
    writesModelMenu && dropdownBinding && !menuAtCap
      ? dropdownBinding
      : writesModelMenu
        ? modelSet.find((m) => m.modelId === defaultModelId)
        : dropdownBinding;
  // 一条常用模型「不可用」，指的是那个模型确实列出了分组、而它绑的那个不在
  // 里面 —— 那是用户的旧选择失效了，要他重选。模型压根没有分组数据（不在
  // /api/pricing 里）不算，那只是我们不知道。
  const bindingsAvailable =
    submittedModels.length > 0 &&
    submittedModels.every((m) => {
      const groups = models.find((a) => a.id === m.modelId)?.billing?.groups;
      return !groups?.length || groups.some((g) => g.id === m.billingGroup);
    });
  // 下拉框的模型和分组已经算进 `submittedModels`，不再要求先点「使用所选模型」。
  // 唯一还要拦住的是第 201 个：那个不能自动写进去。
  const pendingModelEdit = menuAtCap;
  // 列表里有模型、却没有一个是默认：只会在默认模型刚被移走之后出现。
  // 这时不能提交，也不能替用户挑 —— 得他自己点一个。
  const defaultMissing = modelSet.length > 0 && !defaultBinding;
  // 下拉框始终是写进 config 的默认模型。价格卡片跟「正在编辑」的那一粒，
  // 点别的模型不会把默认换掉。
  const priceModelId =
    focusedModelId && submittedModels.some((m) => m.modelId === focusedModelId)
      ? focusedModelId
      : selectedModelId;
  const priceModel =
    models.find((model) => model.id === priceModelId) ?? selectedModel;
  const priceGroup =
    priceModelId === selectedModelId
      ? billingGroup
      : (submittedModels.find((m) => m.modelId === priceModelId)
          ?.billingGroup ?? "");
  const updatePricedGroup = (id: string) => {
    setSelectionReadyKey(selectionKey);
    onReset();
    if (priceModelId === selectedModelId) setBillingGroup(id);
    setModelSet((current) =>
      current.some((m) => m.modelId === priceModelId)
        ? current.map((m) =>
            m.modelId === priceModelId
              ? { modelId: priceModelId, billingGroup: id }
              : m,
          )
        : current,
    );
  };
  /**
   * 把一个模型从列表里拿掉 —— 列表里的「移除」和失败提示里的「移除「X」」
   * 都走这里，两处曾经各写一份、各漏一半。
   *
   * 拿掉的是默认模型时，默认位空出来，不自动让幸存者顶上（理由见
   * `defaultBinding`）。选择器正停在被拿掉的模型上时也要清空：留着它，
   * 下一次提交又会把它自动加回去。
   */
  const removeBinding = (modelId: string) => {
    setModelSet((current) => current.filter((m) => m.modelId !== modelId));
    // 拿掉的是当前默认（下拉框里那一个）时，把选择器清空，不让幸存者顶上。
    // 留着它的话，提交时又会把它自动加回去。
    if (selectedModelId === modelId || defaultModelId === modelId) {
      setDefaultModelId("");
      setSelectedModelId("");
      setBillingGroup("");
    }
    // 移除之后停在页面上，不自动重新接入：挡住的原因不一定是这个模型
    // （另外十六个原因仍然存在），自动重试会把人带偏。
    onReset();
  };
  // 浮层里点中一个模型：新的加进名单。分组走页面已有的 chooseBillingGroup
  // （没有旧分组时用 default，再否则第一个），不再另做一套价格选择。
  // 已经在名单里的，只是把它选成价格卡片正在编辑的那一个。
  const enrollModel = (id: string) => {
    const already =
      id === selectedModelId || modelSet.some((m) => m.modelId === id);
    setFocusedModelId(id);
    if (already) return;
    const accountModel = models.find((m) => m.id === id);
    const group = chooseBillingGroup(accountModel, "");
    const ids = new Set(modelSet.map((m) => m.modelId));
    if (selectedModelId) ids.add(selectedModelId);
    if (ids.has(id) || ids.size >= 200) return;
    setModelSet((current) => {
      const known = new Set(current.map((m) => m.modelId));
      if (selectedModelId) known.add(selectedModelId);
      if (known.has(id) || known.size >= 200) return current;
      const rest = current.filter(
        (m) => m.modelId !== id && m.modelId !== selectedModelId,
      );
      const keptDefault =
        selectedModelId && billingGroup
          ? [{ modelId: selectedModelId, billingGroup }]
          : [];
      return [...keptDefault, ...rest, { modelId: id, billingGroup: group }];
    });
    setSelectionReadyKey(selectionKey);
    onReset();
  };

  const chooseModel = (id: string) => {
    if (pickerAdds) {
      enrollModel(id);
      return;
    }
    setSelectionReadyKey(selectionKey);
    setFocusedModelId(null);
    setSelectedModelId(id);
    const saved = modelSet.find((m) => m.modelId === id);
    setBillingGroup(
      saved?.billingGroup ??
        chooseBillingGroup(
          models.find((m) => m.id === id),
          "",
        ),
    );
    // 下拉框里这个就是默认。已经在名单里，或者还没到 200
    // 的上限，才把它记成默认；超限时不改默认，避免把第
    // 201 个悄悄写进去。
    if (saved || modelSet.length < 200) setDefaultModelId(id);
    onReset();
  };
  const resetSelection = useCallback((closePicker = false) => {
    setSelectedModelId("");
    setBillingGroup("");
    setModelSet([]);
    setDefaultModelId("");
    setSelectionReadyKey(null);
    if (closePicker) {
      setFocusedModelId(null);
      setPickerAdds(false);
      setModelPickerOpen(false);
    }
  }, []);
  const onPickerOpenChange = (next: boolean) => {
    setModelPickerOpen(next);
    if (!next) setPickerAdds(false);
  };
  const startAddingModel = () => {
    setPickerAdds(true);
    setModelPickerOpen(true);
  };
  return {
    selectedModelId,
    billingGroup,
    modelSet,
    modelPickerOpen,
    selectionReadyKey,
    accountModels,
    models,
    selectedModel,
    selectedBillingGroup,
    selectionKey,
    writesModelMenu,
    submittedModels,
    defaultBinding,
    bindingsAvailable,
    pendingModelEdit,
    defaultMissing,
    priceModelId,
    priceModel,
    priceGroup,
    updatePricedGroup,
    removeBinding,
    chooseModel,
    onPickerOpenChange,
    startAddingModel,
    focusPriceModel: setFocusedModelId,
    resetSelection,
  };
}
