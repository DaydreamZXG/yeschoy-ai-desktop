import { Copy, X } from "lucide-react";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogClose,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { useWorkbenchCopy } from "./copy";
import type { AccountProjection } from "../account/session";
import type { AccountLineId } from "../configuration/preview";
import { QQ_GROUP_ID } from "../distribution/qqCommunity";
import { showYeschoyCommunityUi } from "../distribution/yeschoyStation";
// 官方签发的群分享卡片（含二维码 + 群名 + 群号），直接展示，扫码即可加群。
import qqGroupCard from "../assets/brand/qq-group-card.png";

export { QQ_GROUP_ID };

async function copyGroupId(message: string) {
  try {
    await navigator.clipboard.writeText(QQ_GROUP_ID);
    toast.success(message);
  } catch {
    // 剪贴板不可用时 fallback：隐藏 textarea + execCommand
    const ta = document.createElement("textarea");
    ta.value = QQ_GROUP_ID;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    try {
      document.execCommand("copy");
      toast.success(message);
    } catch {
      toast.error(QQ_GROUP_ID);
    }
    document.body.removeChild(ta);
  }
}

export function QQGroupDialog({
  open,
  onOpenChange,
  copy,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  copy: {
    title: string;
    body: string;
    groupIdLabel: string;
    scanHint: string;
    copyGroupId: string;
    groupIdCopied: string;
    close: string;
  };
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="qq-group-dialog" aria-describedby={undefined}>
        <DialogTitle className="qq-group-title">{copy.title}</DialogTitle>
        <DialogDescription className="qq-group-body">
          {copy.body}
        </DialogDescription>
        <div className="qq-group-qr">
          <img
            src={qqGroupCard}
            alt={copy.scanHint}
            width={224}
            height={280}
          />
          <p className="qq-group-scan-hint">{copy.scanHint}</p>
        </div>
        <span className="qq-group-id-row">
          <span className="qq-group-id-label">{copy.groupIdLabel}</span>
          <span className="qq-group-id">{QQ_GROUP_ID}</span>
        </span>
        <div className="qq-group-actions">
          <Button
            variant="default"
            onClick={() => copyGroupId(copy.groupIdCopied)}
          >
            <Copy aria-hidden="true" />
            {copy.copyGroupId}
          </Button>
          <DialogClose asChild>
            <Button variant="ghost">{copy.close}</Button>
          </DialogClose>
        </div>
        <DialogClose asChild>
          <button
            type="button"
            className="qq-group-close-x"
            aria-label={copy.close}
          >
            <X aria-hidden="true" />
          </button>
        </DialogClose>
      </DialogContent>
    </Dialog>
  );
}

/** 交流群入口现在有两个（侧边栏和首页页脚），所以文案在这里取，
    调用方只管开关，不用各自搬一份 copy 过来。 */
export function CommunityGroupDialog({
  open,
  onOpenChange,
  lineId,
  accountProjection,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  lineId: AccountLineId;
  accountProjection: AccountProjection | null;
}) {
  const c = useWorkbenchCopy();
  if (!showYeschoyCommunityUi({ lineId, accountProjection })) {
    return null;
  }
  return (
    <QQGroupDialog
      open={open}
      onOpenChange={onOpenChange}
      copy={{
        title: c.qqGroupTitle,
        body: c.qqGroupBody,
        groupIdLabel: c.qqGroupIdLabel,
        scanHint: c.qqGroupScanHint,
        copyGroupId: c.copyGroupId,
        groupIdCopied: c.groupIdCopied,
        close: c.close,
      }}
    />
  );
}
