import { useEffect, useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { AlertTriangle, Info } from "lucide-react";
import { useTranslation } from "react-i18next";

interface ConfirmDialogProps {
  isOpen: boolean;
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  variant?: "destructive" | "info";
  zIndex?: "base" | "nested" | "alert" | "top";
  /** 可选勾选项：提供 label 即显示，勾选状态经 onConfirm 参数回传 */
  checkboxLabel?: string;
  checkboxDefaultChecked?: boolean;
  pending?: boolean;
  onConfirm: (checkboxChecked: boolean) => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  isOpen,
  title,
  message,
  confirmText,
  cancelText,
  variant = "destructive",
  zIndex = "alert",
  checkboxLabel,
  checkboxDefaultChecked = false,
  pending = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const { t } = useTranslation();
  const [checkboxChecked, setCheckboxChecked] = useState(
    checkboxDefaultChecked,
  );

  useEffect(() => {
    if (isOpen) {
      setCheckboxChecked(checkboxDefaultChecked);
    }
  }, [isOpen, checkboxDefaultChecked]);

  const IconComponent = variant === "info" ? Info : AlertTriangle;

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open && !pending) {
          onCancel();
        }
      }}
    >
      <DialogContent
        className="confirm-dialog"
        data-tone={variant}
        zIndex={zIndex}
      >
        <DialogHeader>
          <DialogTitle className="confirm-dialog-title">
            <IconComponent />
            {title}
          </DialogTitle>
          <DialogDescription>{message}</DialogDescription>
        </DialogHeader>
        {checkboxLabel ? (
          <label className="confirm-dialog-checkbox">
            <Checkbox
              checked={checkboxChecked}
              disabled={pending}
              onCheckedChange={(value) => setCheckboxChecked(value === true)}
            />
            <span>{checkboxLabel}</span>
          </label>
        ) : null}
        <DialogFooter>
          <Button variant="outline" onClick={onCancel} disabled={pending}>
            {cancelText || t("common.cancel")}
          </Button>
          <Button
            className="confirm-dialog-confirm"
            variant={variant === "info" ? "default" : "destructive"}
            disabled={pending}
            onClick={() =>
              // 未渲染勾选框时不得回传 defaultChecked 残留值
              onConfirm(checkboxLabel ? checkboxChecked : false)
            }
          >
            {confirmText || t("common.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
