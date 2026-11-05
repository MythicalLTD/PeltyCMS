import { useEffect, useRef, useState, type FormEvent } from "react";
import { BlockSkeleton } from "@/components/PageSkeleton";
import { Link, useNavigate, useSearchParams } from "react-router";
import { useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Field, FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { Captcha } from "@/components/Captcha";

function useErr() {
  const { t, tCode } = useI18n();
  return (e: unknown) => (e instanceof ApiError ? e.message || tCode(e.code) : t("common.error"));
}

export function ForgotPasswordPage() {
  const { t } = useI18n();
  const msg = useErr();
  const [email, setEmail] = useState("");
  const [sent, setSent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api("/auth/password/forgot", { method: "POST", body: { email } });
      setSent(true);
    } catch (err) {
      setError(msg(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Card>
      <CardHeader>
        <h1 className="text-2xl font-semibold tracking-tight">{t("auth.forgot.title")}</h1>
        <p className="text-sm text-muted-foreground">{t("auth.forgot.subtitle")}</p>
      </CardHeader>
      <CardContent>
        {sent ? (
          <p role="status" data-testid="forgot-sent" className="rounded-md bg-success/10 px-3 py-2 text-sm text-success">{t("auth.forgot.sent")}</p>
        ) : (
          <form onSubmit={submit} className="flex flex-col gap-4" noValidate>
            <FormError message={error} />
            <Field id="email" type="email" autoComplete="email" required label={t("field.email")} placeholder={t("field.email_placeholder")} value={email} onChange={(e) => setEmail(e.target.value)} />
            <Captcha form="password_reset" />
            <Button type="submit" disabled={busy || !email.includes("@")}>{t("auth.forgot.submit")}</Button>
          </form>
        )}
        <p className="mt-4 text-center text-sm">
          <Link to="/login" className="text-primary hover:underline">{t("auth.back_to_login")}</Link>
        </p>
      </CardContent>
    </Card>
  );
}

export function ResetPasswordPage() {
  const { t } = useI18n();
  const msg = useErr();
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const token = params.get("token") ?? "";
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mismatch = pw2.length > 0 && pw !== pw2;
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (pw !== pw2) return;
    setBusy(true);
    setError(null);
    try {
      await api("/auth/password/reset", { method: "POST", body: { token, password: pw } });
      navigate("/login", { replace: true, state: { reset: true } });
    } catch (err) {
      setError(msg(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Card>
      <CardHeader>
        <h1 className="text-2xl font-semibold tracking-tight">{t("auth.reset.title")}</h1>
        <p className="text-sm text-muted-foreground">{t("auth.reset.subtitle")}</p>
      </CardHeader>
      <CardContent>
        {!token ? (
          <FormError message={t("error.invalid_token")} />
        ) : (
          <form onSubmit={submit} className="flex flex-col gap-4" noValidate>
            <FormError message={error} />
            <Field id="password" type="password" autoComplete="new-password" label={t("auth.reset.new_password")} hint={t("field.password_rules")} value={pw} onChange={(e) => setPw(e.target.value)} />
            <Field id="password2" type="password" autoComplete="new-password" label={t("auth.reset.confirm")} value={pw2} onChange={(e) => setPw2(e.target.value)} error={mismatch ? t("auth.reset.mismatch") : undefined} />
            <Button type="submit" disabled={busy || pw.length === 0 || pw !== pw2}>{t("auth.reset.submit")}</Button>
          </form>
        )}
        <p className="mt-4 text-center text-sm">
          <Link to="/forgot-password" className="text-primary hover:underline">{t("auth.reset.request_new")}</Link>
        </p>
      </CardContent>
    </Card>
  );
}

export function VerifyEmailPage() {
  const { t } = useI18n();
  const msg = useErr();
  const qc = useQueryClient();
  const [params] = useSearchParams();
  const token = params.get("token") ?? "";
  const [state, setState] = useState<"pending" | "ok" | string>("pending");
  const once = useRef(false);
  useEffect(() => {
    if (once.current) return;
    once.current = true;
    if (!token) {
      setState(t("error.invalid_token"));
      return;
    }
    api("/auth/email/verify", { method: "POST", body: { token } })
      .then(() => {
        setState("ok");
        void qc.invalidateQueries({ queryKey: ["me"] });
      })
      .catch((e) => setState(msg(e)));
  }, [token, t, msg, qc]);
  return (
    <Card>
      <CardHeader>
        <h1 className="text-2xl font-semibold tracking-tight">{t("auth.verify.title")}</h1>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {state === "pending" && <BlockSkeleton />}
        {state === "ok" && <p role="status" data-testid="verify-ok" className="rounded-md bg-success/10 px-3 py-2 text-sm text-success">{t("auth.verify.done")}</p>}
        {state !== "pending" && state !== "ok" && <FormError message={state} />}
        <Link to="/" className="text-center text-sm text-primary hover:underline">{t("auth.verify.continue")}</Link>
      </CardContent>
    </Card>
  );
}
