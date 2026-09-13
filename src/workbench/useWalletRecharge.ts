import { useCallback } from "react";
import { toast } from "sonner";
import { useWorkbenchCopy } from "./copy";

/**
 * Opens the account wallet and surfaces a toast with a retry action when the
 * wallet page fails to open (previously the failure was silent, PRD 6.2).
 */
export function useWalletRecharge(openWallet: () => Promise<boolean>) {
  const c = useWorkbenchCopy();
  const recharge = useCallback(async (): Promise<void> => {
    if (await openWallet()) return;
    toast.error(c.walletOpenFailed, {
      closeButton: true,
      action: {
        label: c.retry,
        onClick: () => void recharge(),
      },
    });
  }, [openWallet, c]);
  return recharge;
}
