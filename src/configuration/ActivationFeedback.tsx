import type { ReactNode } from "react";
import { CheckCircle2, Info } from "lucide-react";
import { RecoveryNotice } from "./RecoveryNotice";

export function ActivationFeedback({
  kind,
  title,
  children,
}: {
  kind: "success" | "attention" | "error";
  title: string;
  children: ReactNode;
}) {
  if (kind === "error")
    return (
      <RecoveryNotice title={title} className="setup-result is-error">
        {children}
      </RecoveryNotice>
    );
  return (
    <div
      className={`setup-result ${kind === "success" ? "is-success" : "setup-progress"}`}
      role="status"
    >
      {kind === "success" ? (
        <CheckCircle2 aria-hidden="true" />
      ) : (
        <Info aria-hidden="true" />
      )}
      <div>
        <strong>{title}</strong>
        {children}
      </div>
    </div>
  );
}
