import type { ReactNode } from "react";
import { CircleAlert } from "lucide-react";

/** Present a safe explanation and a concrete next action; no raw native errors. */
export function RecoveryNotice({
  title,
  children,
  action,
  className = "",
}: {
  title: string;
  children: ReactNode;
  action?: { label: string; run: () => void; disabled?: boolean };
  className?: string;
}) {
  return (
    <div className={`recovery-notice ${className}`} role="alert">
      <CircleAlert aria-hidden="true" />
      <div>
        <strong>{title}</strong>
        {children}
        {action && (
          <button
            type="button"
            className="subtle-button"
            disabled={action.disabled}
            onClick={action.run}
          >
            {action.label}
          </button>
        )}
      </div>
    </div>
  );
}
