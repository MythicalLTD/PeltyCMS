import { useState } from "react";
import { EyeIcon, EyeSlashIcon, WarningCircleIcon } from "@phosphor-icons/react";
import { Button } from "./ui/button";
import { useI18n } from "@/lib/i18n";
import { Alert, AlertDescription } from "./ui/alert";
import type { InputHTMLAttributes } from "react";
import { Input } from "./ui/input";
import { Label } from "./ui/label";

export function Field({ id, label, error, hint, type, ...props }: InputHTMLAttributes<HTMLInputElement> & { id: string; label: string; error?: string; hint?: string }) {
  const { t } = useI18n();
  const [shown, setShown] = useState(false);
  const isPw = type === "password";
  const describedBy = [error && `${id}-err`, hint && `${id}-hint`].filter(Boolean).join(" ") || undefined;
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor={id}>{label}</Label>
      <div className="relative">
        <Input id={id} type={isPw && shown ? "text" : type} aria-invalid={!!error} aria-describedby={describedBy} className={isPw ? "pr-11" : undefined} {...props} />
        {isPw && (
          <Button type="button" variant="ghost" size="icon" className="absolute top-1/2 right-0.5 size-8 -translate-y-1/2 text-muted-foreground pointer-coarse:size-10"
            aria-label={shown ? t("field.hide_password") : t("field.show_password")} aria-pressed={shown} onClick={() => setShown((v) => !v)}>
            {shown ? <EyeSlashIcon aria-hidden /> : <EyeIcon aria-hidden />}
          </Button>
        )}
      </div>
      {hint && !error && <p id={`${id}-hint`} className="text-sm text-muted-foreground">{hint}</p>}
      {error && (
        <p id={`${id}-err`} className="text-sm text-destructive">
          {error}
        </p>
      )}
    </div>
  );
}

export function FormError({ message }: { message: string | null }) {
  if (!message) return null;
  return (
    <Alert variant="destructive">
      <WarningCircleIcon aria-hidden />
      <AlertDescription>{message}</AlertDescription>
    </Alert>
  );
}
