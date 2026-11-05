import type { ReactNode } from "react";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription,
  AlertDialogFooter, AlertDialogHeader, AlertDialogTitle, AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { buttonVariants } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";

/** Wraps a trigger (usually a Button) with a shadcn confirmation dialog. */
export function ConfirmAction({ children, title, description, confirmLabel, destructive = true, onConfirm, disabled }: {
  children: ReactNode; title?: string; description: string; confirmLabel?: string; destructive?: boolean; onConfirm: () => void; disabled?: boolean;
}) {
  const { t } = useI18n();
  if (disabled) return <>{children}</>;
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>{children}</AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title ?? t("confirm.title")}</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{t("action.cancel")}</AlertDialogCancel>
          <AlertDialogAction className={destructive ? buttonVariants({ variant: "destructive" }) : undefined} onClick={onConfirm}>
            {confirmLabel ?? t("confirm.continue")}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
