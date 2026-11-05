import { useState } from "react";
import { EnvelopeSimpleIcon, XIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";
import { isStaff, useMe } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";

const DISMISS_KEY = "pelty_verify_banner_dismissed";

function wasDismissed(): boolean {
  try {
    return sessionStorage.getItem(DISMISS_KEY) === "1";
  } catch {
    return false;
  }
}

/** Compact one-line nudge for signed-in users with an unverified address; resend + dismiss (per session). */
export function VerifyBanner() {
  const { t } = useI18n();
  const { data: me } = useMe();
  const [state, setState] = useState<"idle" | "busy" | "sent" | "error">("idle");
  const [dismissed, setDismissed] = useState(wasDismissed);
  // Staff accounts are managed by admins, so the reminder is client-only.
  if (!me || me.email_verified !== false || isStaff(me) || dismissed) return null;
  return (
    <output data-testid="verify-banner" className="mb-4 flex items-center gap-2 rounded-md border border-warning/40 bg-warning/10 py-1 pr-1 pl-3 text-sm">
      <EnvelopeSimpleIcon aria-hidden className="size-4 shrink-0" />
      <span className="min-w-0 flex-1 truncate sm:whitespace-normal">
        {state === "sent" || state === "error" ? t(state === "sent" ? "auth.verify.resent" : "common.error") : (
          <>
            <span className="sm:hidden">{t("auth.verify.banner_short")}</span>
            <span className="hidden sm:inline">{t("auth.verify.banner")}</span>
          </>
        )}
      </span>
      {state !== "sent" && (
        <Button
          size="sm"
          variant="ghost"
          className="h-9 shrink-0 px-2 sm:h-8"
          disabled={state === "busy"}
          onClick={async () => {
            setState("busy");
            try {
              await api("/auth/email/resend", { method: "POST" });
              setState("sent");
            } catch {
              setState("error");
            }
          }}
        >
          {t("auth.verify.resend")}
        </Button>
      )}
      <Button
        size="icon"
        variant="ghost"
        className="size-9 shrink-0 sm:size-8"
        aria-label={t("auth.verify.dismiss")}
        data-testid="verify-banner-dismiss"
        onClick={() => {
          try {
            sessionStorage.setItem(DISMISS_KEY, "1");
          } catch {
            /* storage unavailable: dismiss for this render tree only */
          }
          setDismissed(true);
        }}
      >
        <XIcon aria-hidden />
      </Button>
    </output>
  );
}
