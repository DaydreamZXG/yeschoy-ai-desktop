import * as React from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { cn } from "@/lib/utils";
import "./dialog.css";

const Dialog = DialogPrimitive.Root;

const DialogTrigger = DialogPrimitive.Trigger;

const DialogPortal = DialogPrimitive.Portal;

const DialogClose = DialogPrimitive.Close;

type DialogLayer = "base" | "nested" | "alert" | "top";

const DialogOverlay = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Overlay>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Overlay> & {
    zIndex?: DialogLayer;
  }
>(({ className, zIndex = "base", ...props }, ref) => (
  <DialogPrimitive.Overlay
    ref={ref}
    data-dialog-layer={zIndex}
    className={cn("yeschoy-dialog-overlay", className)}
    {...props}
  />
));
DialogOverlay.displayName = DialogPrimitive.Overlay.displayName;

const DialogContent = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content> & {
    zIndex?: DialogLayer;
    variant?: "default" | "fullscreen";
    overlayClassName?: string;
  }
>(
  (
    {
      className,
      children,
      zIndex = "base",
      variant = "default",
      overlayClassName,
      ...props
    },
    ref,
  ) => (
    <DialogPortal>
      <DialogOverlay zIndex={zIndex} className={overlayClassName} />
      <DialogPrimitive.Content
        ref={ref}
        data-dialog-layer={zIndex}
        data-dialog-variant={variant}
        className={cn("yeschoy-dialog-content", className)}
        onInteractOutside={(e) => {
          // 防止点击遮罩层关闭对话框
          e.preventDefault();
        }}
        {...props}
      >
        {children}
      </DialogPrimitive.Content>
    </DialogPortal>
  ),
);
DialogContent.displayName = DialogPrimitive.Content.displayName;

const DialogHeader = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) => (
  <div className={cn("yeschoy-dialog-header", className)} {...props} />
);
DialogHeader.displayName = "DialogHeader";

const DialogFooter = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) => (
  <div className={cn("yeschoy-dialog-footer", className)} {...props} />
);
DialogFooter.displayName = "DialogFooter";

const DialogTitle = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Title>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Title>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Title
    ref={ref}
    className={cn("yeschoy-dialog-title", className)}
    {...props}
  />
));
DialogTitle.displayName = DialogPrimitive.Title.displayName;

const DialogDescription = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Description>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Description>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Description
    ref={ref}
    className={cn("yeschoy-dialog-description", className)}
    {...props}
  />
));
DialogDescription.displayName = DialogPrimitive.Description.displayName;

export {
  Dialog,
  DialogTrigger,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
  DialogDescription,
  DialogClose,
};
