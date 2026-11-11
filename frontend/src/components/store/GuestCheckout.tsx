import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { EnvelopeSimpleIcon, GameControllerIcon } from "@phosphor-icons/react";
import { Captcha } from "@/components/Captcha";
import { Field, FormError } from "@/components/Field";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { api, ApiError, setCsrfToken } from "@/lib/api";
import { emailLooksValid, identityComplete, identityError, shownFields, type GuestAvailability } from "@/lib/guestCheckout";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";
import { useModules } from "@/lib/modules";
import { resetCartId } from "@/lib/store/storefront";

interface Placed { order: { id: string }; invoice: { id: string; status: string }; session: boolean; csrf_token: string | null }

/** Whether the open cart can be bought as a guest (module + store setting). */
export function useGuestCheckout(cartId: string | null) {
  const { enabled } = useModules();
  const on = enabled.has("guest_checkout") && !!cartId;
  const q = useQuery({
    queryKey: ["store", "guest-checkout", cartId],
    queryFn: () => api<GuestAvailability>(`/store/guest-checkout/${cartId}`),
    enabled: on,
    retry: false,
  });
  return on && q.data?.available ? q.data : null;
}

/**
 * Guest checkout step: player name / game identifier + email. A new email
 * pays right away (a session is opened); an email with an unclaimed guest
 * account gets the link by email instead.
 */
export function GuestCheckoutCard({ cartId, avail, fields, country, gateway, validate }: {
  cartId: string;
  avail: GuestAvailability;
  fields: Record<number, Record<string, string>>;
  country: string | null;
  gateway: string | null;
  validate: () => boolean;
}) {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const nav = useNavigate();
  const game = avail.game ?? "generic";
  const shown = shownFields(avail);
  const [email, setEmail] = useState("");
  const [ident, setIdent] = useState<Record<string, string>>({});
  const [touched, setTouched] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sent, setSent] = useState(false);
  const bad = shown.filter((f) => identityError(game, f, ident[f] ?? ""));
  const ok = emailLooksValid(email) && identityComplete(game, ident) && bad.length === 0;
  const go = useMutation({
    mutationFn: async () => {
      const r = await api<Placed>("/store/guest-checkout", { method: "POST", body: { cart_id: cartId, email: email.trim(), identity: ident, fields, country, locale: lang } });
      resetCartId(localStorage);
      if (!r.session) { setSent(true); return; }
      if (r.csrf_token) setCsrfToken(r.csrf_token);
      await qc.invalidateQueries({ queryKey: ["me"] });
      if (gateway && r.invoice.status === "unpaid") {
        const p = await api<{ init: { type: string; url?: string } }>(`/client/invoices/${r.invoice.id}/pay`, { method: "POST", body: { gateway } }).catch(() => null);
        if (p?.init.type === "redirect" && p.init.url) { window.location.assign(p.init.url); return; }
      }
      nav(`/orders/${r.order.id}?placed=1`);
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (sent) {
    return (
      <Card data-testid="guest-sent">
        <CardHeader>
          <CardTitle className="flex items-center gap-2"><EnvelopeSimpleIcon aria-hidden className="size-5 text-primary" />{t("guest.sent_title")}</CardTitle>
          <CardDescription>{t("guest.sent_desc", { email: email.trim() })}</CardDescription>
        </CardHeader>
      </Card>
    );
  }
  const submit = () => { setTouched(true); setError(null); if (ok && validate()) go.mutate(); };
  return (
    <Card data-testid="guest-checkout">
      <CardHeader>
        <CardTitle className="flex items-center gap-2"><GameControllerIcon aria-hidden className="size-5 text-primary" />{t("guest.title")}</CardTitle>
        <CardDescription>{t("guest.desc")}</CardDescription>
      </CardHeader>
      <CardContent>
        <form className="grid gap-3" noValidate onSubmit={(e) => { e.preventDefault(); submit(); }}>
          <FormError message={error} />
          <div className="grid gap-3 *:min-w-0 sm:grid-cols-2">
            {shown.map((f) => (
              <Field key={f} id={`guest-${f}`} label={t(`game.field.${f}` as TranslationKey)} autoComplete="off" spellCheck={false}
                required={game === "minecraft_java" && f === "username"}
                value={ident[f] ?? ""} onChange={(e) => setIdent((s) => ({ ...s, [f]: e.target.value }))}
                error={touched && bad.includes(f) ? t("error.game_identity_invalid") : undefined}
                hint={f === "username" && game === "minecraft_java" && avail.verify_minecraft ? t("guest.mojang_hint") : undefined} />
            ))}
          </div>
          {touched && !identityComplete(game, ident) && <p className="text-sm text-destructive">{t("error.guest_identity_required")}</p>}
          <Field id="guest-email" type="email" required autoComplete="email" label={t("field.email")} value={email} onChange={(e) => setEmail(e.target.value)} error={touched && !emailLooksValid(email) ? t("error.invalid_email") : undefined}
            hint={t("guest.email_hint")} />
          <Captcha form="checkout" />
          <Button type="submit" disabled={go.isPending}>{t("guest.continue")}</Button>
        </form>
      </CardContent>
    </Card>
  );
}
