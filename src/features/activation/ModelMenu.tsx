import type { AccountModel } from "../../account/session";
import type {
  ActivationToolId,
  ModelBinding,
} from "../../configuration/activation";
import type { ConfigurationCopy } from "../../configuration/copy";
import { groupLabel } from "../../configuration/BillingGroupPicker";
import { modelDisplayName, shortModelId } from "../../model-profiles/profile";

/** Presentation only; choosing/editing/removing a binding belongs to the selection hook. */
export function ModelMenu({
  activationToolId,
  models,
  submittedModels,
  defaultBinding,
  priceModelId,
  disabled,
  addDisabled,
  focusPriceModel,
  removeBinding,
  startAddingModel,
  g,
}: {
  activationToolId: ActivationToolId;
  models: readonly AccountModel[];
  submittedModels: readonly ModelBinding[];
  defaultBinding?: ModelBinding;
  priceModelId: string;
  disabled: boolean;
  addDisabled: boolean;
  focusPriceModel: (modelId: string) => void;
  removeBinding: (modelId: string) => void;
  startAddingModel: () => void;
  g: ConfigurationCopy;
}) {
  return (
    <section className="model-menu-row" aria-label={g.favoriteModels}>
      <p className="model-menu-note">
        {activationToolId === "codex_desktop"
          ? g.menuModelsCodex
          : g.menuModelsApp}
      </p>
      <ul className="model-menu-chips">
        {submittedModels.map((m) => {
          const bindingModel = models.find((a) => a.id === m.modelId);
          const groups = bindingModel?.billing?.groups;
          const available =
            !groups?.length ||
            groups.some((group) => group.id === m.billingGroup);
          const isDefault = defaultBinding?.modelId === m.modelId;
          const isEditing = priceModelId === m.modelId;
          const shortGroup = groupLabel(m.billingGroup, g.defaultGroup);
          const groupText = available
            ? shortGroup
            : `${shortGroup} · ${g.bindingUnavailable}`;
          return (
            <li key={m.modelId}>
              <span
                className={`model-menu-chip${isDefault ? " is-default" : ""}${isEditing ? " is-editing" : ""}`}
                data-unavailable={available ? undefined : true}
              >
                <button
                  type="button"
                  className="model-menu-chip-select"
                  aria-pressed={isEditing}
                  disabled={disabled}
                  title={`${m.modelId} · ${groupText}`}
                  onClick={() => focusPriceModel(m.modelId)}
                >
                  <strong>{modelDisplayName(m.modelId)}</strong>
                  {modelDisplayName(m.modelId) !== m.modelId && (
                    <code>{shortModelId(m.modelId)}</code>
                  )}
                  <small>{groupText}</small>
                  {isDefault && <em>{g.defaultBadge}</em>}
                </button>
                {!isDefault && (
                  <button
                    type="button"
                    className="model-menu-remove"
                    disabled={disabled}
                    aria-label={g.removeModelAria.replace(
                      "{{model}}",
                      m.modelId,
                    )}
                    onClick={() => removeBinding(m.modelId)}
                  >
                    {g.removeAction}
                  </button>
                )}
              </span>
            </li>
          );
        })}
        <li>
          <button
            type="button"
            className="model-menu-add"
            disabled={addDisabled}
            onClick={startAddingModel}
          >
            {g.addMenuModel}
          </button>
        </li>
      </ul>
      {submittedModels.length > 1 && (
        <small className="model-menu-footnote">{g.modelSetNote}</small>
      )}
    </section>
  );
}
