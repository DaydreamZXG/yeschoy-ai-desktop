import type { TFunction } from "i18next";
import { CircleStop, LoaderCircle } from "lucide-react";
import type { ActivationProgress as Progress } from "../../configuration/activation";
import type { ConfigurationCopy } from "../../configuration/copy";

export function ActivationProgress({
  activationProgress,
  activationProgressText,
  activationCancelRequested,
  cancelFailed,
  cancelActivation,
  settingUp,
  t,
  g,
}: {
  activationProgress: Progress | null;
  activationProgressText: string;
  activationCancelRequested: boolean;
  cancelFailed: boolean;
  cancelActivation: () => Promise<void>;
  settingUp: string;
  t: TFunction;
  g: ConfigurationCopy;
}) {
  return (
    <div className="setup-progress" role="status" aria-live="polite">
      <LoaderCircle className="is-spinning" aria-hidden="true" />
      <p>
        <strong>{settingUp}</strong>
        <span>{activationProgressText}</span>
        {activationProgress && (
          <span>
            {g.progressStep
              .replace("{{done}}", String(activationProgress.completedSteps))
              .replace("{{total}}", String(activationProgress.totalSteps))}
          </span>
        )}
        {cancelFailed && (
          <span role="alert">{t("yeschoyDaily.cancelFailed")}</span>
        )}
        <span>{g.progressNote}</span>
        <button
          type="button"
          className="subtle-button"
          onClick={() => void cancelActivation()}
          disabled={activationCancelRequested}
        >
          <CircleStop aria-hidden="true" />
          {activationCancelRequested ? g.cancelInProgress : g.cancelSetup}
        </button>
      </p>
    </div>
  );
}
