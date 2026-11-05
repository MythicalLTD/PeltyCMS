import { useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { ShieldCheckIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { BlockSkeleton } from "@/components/PageSkeleton";
import { FormError } from "@/components/Field";
import { api, ApiError, setCsrfToken, type Me } from "@/lib/api";
import { isStaff } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";

/** Login step: hand the challenge to Duo and leave for the Universal Prompt. */
export function DuoStep({ challenge, onRestart, go = (u) => window.location.assign(u) }: { challenge: string; onRestart: () => void; go?: (url: string) => void }) {
  const { t, tCode } = useI18n();
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const start = async () => {
    setBusy(true);
    setErr(null);
    try {
      const r = await api<{ url: string }>("/auth/duo/start", { method: "POST", body: { challenge } });
      if (!r.url.startsWith("https://")) throw new Error("bad url");
      go(r.url);
    } catch (e) {
      setErr(e instanceof ApiError ? tCode(e.code) : t("common.error"));
      setBusy(false);
    }
  };
  return (
    <Card>
      <CardHeader>
        <h1 className="flex items-center gap-2 text-2xl font-semibold tracking-tight"><ShieldCheckIcon aria-hidden />{t("duo.title")}</h1>
        <p className="text-sm text-muted-foreground">{t("duo.step_desc")}</p>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <FormError message={err} />
        <Button onClick={() => void start()} disabled={busy}>{t("duo.continue")}</Button>
        <Button variant="ghost" onClick={onRestart}>{t("common.back")}</Button>
      </CardContent>
    </Card>
  );
}

/** `/auth/duo/callback?state&duo_code` (Duo redirects here). */
export function DuoCallbackPage() {
  const { t, tCode } = useI18n();
  const [params] = useSearchParams();
  const nav = useNavigate();
  const qc = useQueryClient();
  const state = params.get("state") ?? "";
  const code = params.get("duo_code") ?? params.get("code") ?? "";
  const missing = !state || !code;
  const finish = useQuery({
    queryKey: ["auth", "duo-finish", state, code],
    queryFn: async () => {
      const me = await api<Me>("/auth/duo/finish", { method: "POST", body: { state, duo_code: code } });
      setCsrfToken(me.csrf_token);
      qc.setQueryData(["me"], me);
      nav(isStaff(me) ? "/admin" : "/", { replace: true });
      return me;
    },
    enabled: !missing,
    retry: false,
    staleTime: Infinity,
  });
  const err = missing ? t("error.duo_denied") : finish.isError ? (finish.error instanceof ApiError ? tCode(finish.error.code) : t("common.error")) : null;
  return (
    <Card>
      <CardHeader><h1 className="text-2xl font-semibold tracking-tight">{t("duo.title")}</h1></CardHeader>
      <CardContent className="flex flex-col gap-3">
        {err ? <><FormError message={err} /><Button variant="outline" onClick={() => nav("/login", { replace: true })}>{t("auth.login.title")}</Button></> : <><p className="text-sm text-muted-foreground">{t("duo.callback")}</p><BlockSkeleton lines={1} /></>}
      </CardContent>
    </Card>
  );
}

interface DuoCfg { enabled: boolean; client_id: string; api_host: string; scope: "staff" | "all"; has_secret: boolean }

export function DuoCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const key = ["admin", "duo"];
  const { data } = useQuery({ queryKey: key, queryFn: () => api<DuoCfg>("/admin/security/duo") });
  const [f, setF] = useState<DuoCfg & { client_secret: string } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [seen, setSeen] = useState(data);
  if (data !== seen) {
    setSeen(data);
    if (data !== undefined) setF({ ...data, client_secret: "" });
  }
  const onErr = (e: unknown) => setErr(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const save = useMutation({ mutationFn: (b: DuoCfg & { client_secret: string }) => api<DuoCfg>("/admin/security/duo", { method: "PUT", body: { enabled: b.enabled, client_id: b.client_id, api_host: b.api_host, scope: b.scope, client_secret: b.client_secret } }), onSuccess: (r) => { qc.setQueryData(key, r); setErr(null); toast.success(t("common.saved")); }, onError: onErr });
  const test = useMutation({ mutationFn: () => api("/admin/security/duo/test", { method: "POST" }), onSuccess: () => { setErr(null); toast.success(t("duo.test_ok")); }, onError: onErr });
  if (!f) return <BlockSkeleton />;
  const set = (p: Partial<typeof f>) => setF({ ...f, ...p });
  return (
    <Card>
      <CardHeader><CardTitle>{t("duo.title")}</CardTitle><CardDescription>{t("duo.desc")}</CardDescription></CardHeader>
      <CardContent>
        <form className="grid gap-3 sm:grid-cols-2" onSubmit={(e) => { e.preventDefault(); save.mutate(f); }}>
          <div className="flex items-center gap-2 sm:col-span-2"><Switch id="duo-on" checked={f.enabled} onCheckedChange={(v) => set({ enabled: v })} /><Label htmlFor="duo-on">{t("duo.enabled")}</Label></div>
          <div className="grid gap-1"><Label htmlFor="duo-cid">{t("duo.client_id")}</Label><Input id="duo-cid" value={f.client_id} maxLength={20} onChange={(e) => set({ client_id: e.target.value })} className="font-mono" /></div>
          <div className="grid gap-1"><Label htmlFor="duo-sec">{t("duo.client_secret")}</Label><Input id="duo-sec" type="password" autoComplete="off" value={f.client_secret} maxLength={40} placeholder={f.has_secret ? t("duo.secret_saved") : ""} onChange={(e) => set({ client_secret: e.target.value })} /></div>
          <div className="grid gap-1"><Label htmlFor="duo-host">{t("duo.api_host")}</Label><Input id="duo-host" value={f.api_host} placeholder="api-xxxxxxxx.duosecurity.com" onChange={(e) => set({ api_host: e.target.value })} /></div>
          <div className="grid gap-1"><Label htmlFor="duo-scope">{t("duo.scope")}</Label>
            <Select value={f.scope} onValueChange={(v) => set({ scope: v as DuoCfg["scope"] })}><SelectTrigger id="duo-scope"><SelectValue /></SelectTrigger>
              <SelectContent><SelectItem value="staff">{t("duo.scope_staff")}</SelectItem><SelectItem value="all">{t("duo.scope_all")}</SelectItem></SelectContent></Select></div>
          <p className="text-xs text-muted-foreground break-all sm:col-span-2">{t("duo.redirect")}: <span className="font-mono">{window.location.origin}/auth/duo/callback</span></p>
          <div className="sm:col-span-2"><FormError message={err} /></div>
          <div className="flex flex-wrap gap-2 sm:col-span-2">
            <Button type="submit" disabled={save.isPending}>{t("common.save")}</Button>
            <Button type="button" variant="outline" disabled={test.isPending || !data?.has_secret} onClick={() => test.mutate()}>{t("duo.test")}</Button>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}
