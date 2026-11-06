import { useEffect, useRef, useState } from "react";
import { Link, useLocation, useNavigate, useSearchParams } from "react-router";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import * as z from "zod/mini";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Field, FormError } from "@/components/Field";
import { useAuthActions, isStaff } from "@/lib/auth";
import type { Me } from "@/lib/api";
import { ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { Captcha } from "@/components/Captcha";
import { SsoButtons } from "@/components/account/SsoButtons";
import { FingerprintIcon } from "@phosphor-icons/react";
import { useQueryClient } from "@tanstack/react-query";
import { setCsrfToken } from "@/lib/api";
import { conditionalMediationAvailable, passkeyDiscoverLogin, passkeyLogin } from "@/lib/passkeys";
import { usePasskeysAvailable } from "@/lib/admin/general";
import { MFA_CHALLENGE_KEY, MFA_DUO_KEY, MFA_METHODS_KEY } from "@/pages/auth/Verify2fa";

const schema = z.object({ email: z.email(), password: z.string().check(z.minLength(1)) });
type Values = z.infer<typeof schema>;

export function LoginPage() {
  const { t, tCode } = useI18n();
  const { login } = useAuthActions();
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const loc = useLocation();
  const oauthErr = params.get("oauth_error") ?? params.get("saml_error");
  const [error, setError] = useState<string | null>(oauthErr ? tCode(`error.${oauthErr.replace(/[^a-z_]/g, "")}`) : null);
  const { register, handleSubmit, formState, getValues } = useForm<Values>({ resolver: zodResolver(schema) });
  const qc = useQueryClient();
  const [pkBusy, setPkBusy] = useState(false);
  const pk = usePasskeysAvailable();

  // Legacy ?mfa_challenge=… links → FeatherPanel-shaped /auth/verify-2fa
  useEffect(() => {
    const ch = params.get("mfa_challenge");
    if (!ch) return;
    sessionStorage.setItem(MFA_CHALLENGE_KEY, ch);
    sessionStorage.setItem(MFA_METHODS_KEY, params.get("methods") ?? "totp");
    if (params.get("duo") === "1") sessionStorage.setItem(MFA_DUO_KEY, "1");
    const email = params.get("email") ?? "";
    navigate(`/auth/verify-2fa?email=${encodeURIComponent(email)}${params.get("duo") === "1" ? "&duo=1" : ""}`, { replace: true });
  }, [params, navigate]);

  const done = (me: Me) => {
    const from = (loc.state as { from?: string } | null)?.from;
    navigate(from ?? (isStaff(me) ? "/admin" : "/"), { replace: true });
  };

  const onSubmit = handleSubmit(async (v) => {
    setError(null);
    try {
      const r = await login(v.email, v.password);
      if (r.challenge) {
        sessionStorage.setItem(MFA_CHALLENGE_KEY, r.challenge);
        sessionStorage.setItem(MFA_METHODS_KEY, (r.methods ?? ["totp"]).join(","));
        if (r.duo === true) sessionStorage.setItem(MFA_DUO_KEY, "1");
        else sessionStorage.removeItem(MFA_DUO_KEY);
        // Same link shape as FeatherPanel: /auth/verify-2fa?email=…
        navigate(`/auth/verify-2fa?email=${encodeURIComponent(v.email)}${r.duo ? "&duo=1" : ""}`, { replace: true, state: loc.state });
        return;
      }
      if (r.me) done(r.me);
    } catch (e) {
      setError(e instanceof ApiError ? e.message || tCode(e.code) : t("common.error"));
    }
  });

  const signedIn = (me: Me) => {
    setCsrfToken(me.csrf_token);
    qc.setQueryData(["me"], me);
    done(me);
  };

  // Passkey autofill on the e-mail field (conditional UI). Aborted when the
  // user picks the explicit button or leaves the page.
  const autofill = useRef<AbortController | null>(null);
  useEffect(() => {
    if (!pk) return;
    let live = true;
    void conditionalMediationAvailable().then((ok) => {
      if (!ok || !live) return undefined;
      const ac = new AbortController();
      autofill.current = ac;
      return passkeyDiscoverLogin<Me>({ conditional: true, signal: ac.signal }).then(signedIn).catch((e: unknown) => {
        if (!ac.signal.aborted && e instanceof ApiError) setError(tCode(e.code));
      });
    });
    return () => { live = false; autofill.current?.abort(); autofill.current = null; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pk]);

  /** Typed e-mail → that account's passkeys; empty → any saved passkey. */
  const withPasskey = async () => {
    const email = getValues("email")?.trim();
    autofill.current?.abort();
    autofill.current = null;
    setError(null);
    setPkBusy(true);
    try {
      signedIn(email ? await passkeyLogin<Me>(email) : await passkeyDiscoverLogin<Me>());
    } catch (e) {
      setError(e instanceof ApiError ? tCode(e.code) : t("passkeys.cancelled"));
    } finally {
      setPkBusy(false);
    }
  };

  return (
    <Card>
      <CardHeader>
        <h1 className="text-2xl font-semibold tracking-tight">{t("auth.login.title")}</h1>
        <p className="text-sm text-muted-foreground">{t("auth.login.subtitle")}</p>
      </CardHeader>
      <CardContent>
        <form onSubmit={onSubmit} className="flex flex-col gap-4" noValidate>
          {(loc.state as { registered?: boolean } | null)?.registered && (
            <p className="rounded-md bg-success/10 px-3 py-2 text-sm text-success">{t("auth.register.success")}</p>
          )}
          {(loc.state as { reset?: boolean } | null)?.reset && (
            <p className="rounded-md bg-success/10 px-3 py-2 text-sm text-success">{t("auth.reset.done")}</p>
          )}
          <FormError message={error} />
          <Field id="email" type="email" autoComplete="username webauthn" label={t("field.email")} placeholder={t("field.email_placeholder")} error={formState.errors.email && t("validation.email")} {...register("email")} />
          <Field id="password" type="password" autoComplete="current-password" label={t("field.password")} placeholder="••••••••••••" error={formState.errors.password && t("validation.required")} {...register("password")} />
          <Link to="/forgot-password" className="-mt-2 self-end text-sm text-primary hover:underline">{t("auth.login.forgot")}</Link>
          <Captcha form="login" />
          <Button type="submit" disabled={formState.isSubmitting}>{t("auth.login.submit")}</Button>
          {pk && <Button type="button" variant="outline" disabled={pkBusy} onClick={() => void withPasskey()}><FingerprintIcon aria-hidden />{t("passkeys.sign_in")}</Button>}
          {pk && <p className="text-muted-foreground text-center text-xs">{t("passkeys.discover_hint")}</p>}
          <SsoButtons />
          <p className="text-center text-sm text-muted-foreground">
            {t("auth.login.no_account")}{" "}
            <Link to="/register" className="text-primary hover:underline">{t("auth.login.register_link")}</Link>
          </p>
        </form>
      </CardContent>
    </Card>
  );
}
