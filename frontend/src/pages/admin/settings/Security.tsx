import { DuoCard } from "@/components/account/Duo";
import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FileTextIcon, ShieldCheckIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Link } from "react-router";
import { Field, FormError } from "@/components/Field";
import { PageHeader } from "@/components/PageHeader";
import { SecurityQuestionsAdminCard } from "@/components/account/SecurityQuestions";
import { ListCard } from "@/components/ListPage";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

interface Policy { password_min_length: number; require_upper: boolean; require_lower: boolean; require_digit: boolean; require_symbol: boolean; session_idle_minutes: number | null; session_absolute_hours: number | null; login_rate_per_minute: number | null; admin_reauth_minutes: number | null; password_max_age_days: number | null; login_alerts: boolean }
interface CaptchaPolicy { mode: string; provider: string; forms: string[] }
interface Addon { id: string; kind: string; name: string; status: string }
interface Tos { id: string; version: string; url: string; summary: string; published_at: string }
interface Rule { id: string; cidr: string; kind: string; reason: string; expires_at: string | null; created_at: string }

function useErr() {
  const { t, tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

function PolicyCard() {
  const { t } = useI18n();
  const onErr = useErr();
  const q = useQuery({ queryKey: ["admin", "security-policy"], queryFn: () => api<{ policy: Policy; defaults: { session_idle_minutes: number; session_absolute_hours: number; login_rate_per_minute: number } }>("/admin/security/policy") });
  const [p, setP] = useState<Policy | null>(null);
  useEffect(() => { if (q.data) setP(q.data.policy); }, [q.data]);
  const save = useMutation({ mutationFn: (b: Policy) => api("/admin/security/policy", { method: "PUT", body: b }), onSuccess: () => toast.success(t("security.saved")), onError: onErr });
  if (!p) return null;
  const opt = (k: keyof Policy) => (e: React.ChangeEvent<HTMLInputElement>) => setP({ ...p, [k]: e.target.value === "" ? null : Number(e.target.value) });
  const d = q.data?.defaults;
  return (
    <Card>
      <CardHeader><CardTitle>{t("security.policy")}</CardTitle><CardDescription>{t("security.policy_hint")}</CardDescription></CardHeader>
      <CardContent>
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(p); }}>
          <div className="grid gap-4 *:min-w-0 sm:grid-cols-2">
            <Field id="pw-min" type="number" min={8} max={128} label={t("security.pw_min")} value={p.password_min_length} onChange={(e) => setP({ ...p, password_min_length: Number(e.target.value) })} />
            <Field id="rate" type="number" min={1} label={t("security.login_rate")} placeholder={String(d?.login_rate_per_minute ?? "")} value={p.login_rate_per_minute ?? ""} onChange={opt("login_rate_per_minute")} />
            <Field id="idle" type="number" min={5} label={t("security.session_idle")} placeholder={String(d?.session_idle_minutes ?? "")} value={p.session_idle_minutes ?? ""} onChange={opt("session_idle_minutes")} />
            <Field id="abs" type="number" min={1} label={t("security.session_absolute")} placeholder={String(d?.session_absolute_hours ?? "")} value={p.session_absolute_hours ?? ""} onChange={opt("session_absolute_hours")} />
            <Field id="reauth" type="number" min={1} label={t("security.admin_reauth")} hint={t("security.admin_reauth_hint")} value={p.admin_reauth_minutes ?? ""} onChange={opt("admin_reauth_minutes")} />
            <Field id="pw-age" type="number" min={1} label={t("security.pw_max_age")} hint={t("security.pw_max_age_hint")} value={p.password_max_age_days ?? ""} onChange={opt("password_max_age_days")} />
          </div>
          <div className="grid gap-3 sm:grid-cols-2">
            {(["require_upper", "require_lower", "require_digit", "require_symbol", "login_alerts"] as const).map((k) => (
              <div key={k} className="flex items-center gap-2"><Switch id={k} checked={p[k]} onCheckedChange={(v) => setP({ ...p, [k]: v })} /><Label htmlFor={k}>{t(`security.${k}`)}</Label></div>
            ))}
          </div>
          <div><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></div>
        </form>
      </CardContent>
    </Card>
  );
}

function CaptchaCard() {
  const { t, tCode } = useI18n();
  const onErr = useErr();
  const q = useQuery({ queryKey: ["admin", "captcha"], queryFn: () => api<{ policy: CaptchaPolicy; forms: string[]; modes: string[] }>("/admin/security/captcha") });
  const addons = useQuery({ queryKey: ["admin", "addons"], queryFn: () => api<{ items: Addon[] }>("/admin/addons") });
  const [c, setC] = useState<CaptchaPolicy | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => { if (q.data) setC(q.data.policy); }, [q.data]);
  const save = useMutation({
    mutationFn: (b: CaptchaPolicy) => api("/admin/security/captcha", { method: "PUT", body: b }),
    onSuccess: () => { setErr(null); toast.success(t("security.saved")); },
    onError: (e) => { setErr(e instanceof ApiError ? tCode(e.code) : t("error.internal")); onErr(e); },
  });
  if (!c || !q.data) return null;
  const providers = (addons.data?.items ?? []).filter((a) => a.kind === "captcha");
  return (
    <Card>
      <CardHeader><CardTitle>{t("security.captcha")}</CardTitle><CardDescription>{t("security.captcha_hint")} <Link className="text-primary hover:underline" to="/admin/addons">{t("nav.addons")}</Link></CardDescription></CardHeader>
      <CardContent>
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(c); }}>
          <div className="grid gap-4 *:min-w-0 sm:grid-cols-2">
            <div className="flex flex-col gap-2">
              <Label>{t("security.captcha_mode")}</Label>
              <Select value={c.mode} onValueChange={(v) => setC({ ...c, mode: v })}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>{q.data.modes.map((m) => <SelectItem key={m} value={m}>{tCode(`security.captcha_mode_${m}`)}</SelectItem>)}</SelectContent>
              </Select>
            </div>
            <div className="flex flex-col gap-2">
              <Label>{t("security.captcha_provider")}</Label>
              <Select value={c.provider || "none"} onValueChange={(v) => setC({ ...c, provider: v === "none" ? "" : v })}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="none">-</SelectItem>
                  {providers.map((a) => <SelectItem key={a.id} value={a.id} disabled={a.status !== "enabled"}>{a.name}</SelectItem>)}
                </SelectContent>
              </Select>
            </div>
          </div>
          <div className="grid gap-2 sm:grid-cols-2">
            {q.data.forms.map((f) => (
              <label key={f} className="flex items-center gap-2 text-sm">
                <Checkbox checked={c.forms.includes(f)} onCheckedChange={(v) => setC({ ...c, forms: v ? [...c.forms, f] : c.forms.filter((x) => x !== f) })} />
                {tCode(`security.form_${f}`)}
              </label>
            ))}
          </div>
          <FormError message={err} />
          <div><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></div>
        </form>
      </CardContent>
    </Card>
  );
}

function IpRulesCard() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const onErr = useErr();
  const q = useQuery({ queryKey: ["admin", "ip-rules"], queryFn: () => api<{ rules: Rule[]; your_ip: string | null }>("/admin/security/ip-rules") });
  const [cidr, setCidr] = useState("");
  const [kind, setKind] = useState("ban");
  const [reason, setReason] = useState("");
  const [hours, setHours] = useState("");
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "ip-rules"] });
  const add = useMutation({
    mutationFn: () => api("/admin/security/ip-rules", { method: "POST", body: { cidr: cidr.trim(), kind, reason, hours: hours ? Number(hours) : null } }),
    onSuccess: () => { setCidr(""); setReason(""); setHours(""); refresh(); }, onError: onErr,
  });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/security/ip-rules/${id}`, { method: "DELETE" }), onSuccess: refresh, onError: onErr });
  return (
    <div className="flex flex-col gap-3">
    <Card>
      <CardHeader><CardTitle>{t("security.ip_rules")}</CardTitle><CardDescription>{t("security.ip_rules_hint", { ip: q.data?.your_ip ?? "-" })}</CardDescription></CardHeader>
      <CardContent>
        <form className="grid gap-4 *:min-w-0 sm:grid-cols-2 lg:grid-cols-5 lg:items-end" onSubmit={(e) => { e.preventDefault(); if (cidr.trim()) add.mutate(); }}>
          <Field id="cidr" label={t("security.cidr")} placeholder="203.0.113.0/24" value={cidr} onChange={(e) => setCidr(e.target.value)} />
          <div className="flex flex-col gap-2">
            <Label>{t("security.rule_kind")}</Label>
            <Select value={kind} onValueChange={setKind}>
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent><SelectItem value="ban">{t("security.kind_ban")}</SelectItem><SelectItem value="admin_allow">{t("security.kind_admin_allow")}</SelectItem></SelectContent>
            </Select>
          </div>
          <Field id="reason" label={t("security.reason")} value={reason} onChange={(e) => setReason(e.target.value)} />
          <Field id="hours" type="number" min={1} label={t("security.hours")} hint={t("security.hours_hint")} value={hours} onChange={(e) => setHours(e.target.value)} />
          <Button type="submit" disabled={add.isPending || !cidr.trim()}>{t("security.add_rule")}</Button>
        </form>
      </CardContent>
    </Card>
    <ListCard<Rule>
      rows={q.data?.rules} rowKey={(r) => r.id} loading={q.isLoading} emptyIcon={ShieldCheckIcon} emptyTitle={t("security.no_rules")}
      columns={[
        { header: t("security.cidr"), primary: true, cell: (r) => <span className="font-mono text-sm">{r.cidr}</span> },
        { header: t("security.rule_kind"), cell: (r) => <Badge variant={r.kind === "ban" ? "destructive" : "secondary"}>{tCode(`security.kind_${r.kind}`)}</Badge> },
        { header: t("security.reason"), cell: (r) => r.reason || "-" },
        { header: t("security.expires"), cell: (r) => (r.expires_at ? formatDateTime(r.expires_at, lang) : t("security.never")) },
        { header: "", cell: (r) => <Button size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => del.mutate(r.id)}><TrashIcon /></Button> },
      ]}
    />
    </div>
  );
}

function TosCard() {
  const { t, lang } = useI18n();
  const qc = useQueryClient();
  const onErr = useErr();
  const q = useQuery({ queryKey: ["admin", "tos"], queryFn: () => api<{ items: { version: Tos; acceptances: number }[] }>("/admin/security/tos") });
  const [version, setVersion] = useState("");
  const [url, setUrl] = useState("");
  const [summary, setSummary] = useState("");
  const pub = useMutation({
    mutationFn: () => api("/admin/security/tos", { method: "POST", body: { version: version.trim(), url: url.trim(), summary } }),
    onSuccess: () => { setVersion(""); setUrl(""); setSummary(""); toast.success(t("security.saved")); void qc.invalidateQueries({ queryKey: ["admin", "tos"] }); },
    onError: onErr,
  });
  return (
    <div className="flex flex-col gap-3">
    <Card>
      <CardHeader><CardTitle>{t("security.tos")}</CardTitle><CardDescription>{t("security.tos_hint")}</CardDescription></CardHeader>
      <CardContent>
        <form className="grid gap-4 *:min-w-0 sm:grid-cols-2 lg:grid-cols-4 lg:items-end" onSubmit={(e) => { e.preventDefault(); if (version.trim()) pub.mutate(); }}>
          <Field id="tos-version" label={t("security.tos_version")} placeholder="2026-10" value={version} onChange={(e) => setVersion(e.target.value)} />
          <Field id="tos-url" label={t("security.tos_url")} placeholder="/terms" value={url} onChange={(e) => setUrl(e.target.value)} />
          <Field id="tos-summary" label={t("security.tos_summary")} value={summary} onChange={(e) => setSummary(e.target.value)} />
          <Button type="submit" disabled={pub.isPending || !version.trim()}>{t("security.tos_publish")}</Button>
        </form>
      </CardContent>
    </Card>
    <ListCard<{ version: Tos; acceptances: number }>
      rows={q.data?.items} rowKey={(r) => r.version.id} loading={q.isLoading} emptyIcon={FileTextIcon} emptyTitle={t("security.tos_none")}
      columns={[
        { header: t("security.tos_version"), primary: true, cell: (r) => r.version.url ? <a className="text-primary hover:underline" href={r.version.url} target="_blank" rel="noreferrer">{r.version.version}</a> : r.version.version },
        { header: t("security.tos_published"), cell: (r) => formatDateTime(r.version.published_at, lang) },
        { header: t("security.tos_acceptances"), cell: (r) => r.acceptances },
      ]}
    />
    </div>
  );
}

type Banned = { domain: string; reason: string; hits: number; created_at: string };

function BannedEmailsCard() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const onErr = useErr();
  const q = useQuery({ queryKey: ["admin", "banned-emails"], queryFn: () => api<{ items: Banned[] }>("/admin/banned-emails") });
  const [domain, setDomain] = useState("");
  const [reason, setReason] = useState("");
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "banned-emails"] });
  const add = useMutation({ mutationFn: () => api("/admin/banned-emails", { method: "POST", body: { domain: domain.trim(), reason } }), onSuccess: () => { setDomain(""); setReason(""); refresh(); }, onError: onErr });
  const del = useMutation({ mutationFn: (d: string) => api(`/admin/banned-emails/${encodeURIComponent(d)}`, { method: "DELETE" }), onSuccess: refresh, onError: onErr });
  return (
    <div className="flex flex-col gap-3">
    <Card>
      <CardHeader><CardTitle>{t("banned.title")}</CardTitle><CardDescription>{t("banned.hint")}</CardDescription></CardHeader>
      <CardContent>
        <form className="grid gap-4 *:min-w-0 sm:grid-cols-3 sm:items-end" onSubmit={(e) => { e.preventDefault(); if (domain.trim()) add.mutate(); }}>
          <Field id="ban-domain" label={t("banned.domain")} placeholder="mailinator.com" value={domain} onChange={(e) => setDomain(e.target.value)} />
          <Field id="ban-reason" label={t("banned.reason")} value={reason} onChange={(e) => setReason(e.target.value)} />
          <Button type="submit" disabled={add.isPending || !domain.trim()}>{t("banned.add")}</Button>
        </form>
      </CardContent>
    </Card>
    <ListCard<Banned>
      rows={q.data?.items} rowKey={(r) => r.domain} loading={q.isLoading} emptyIcon={ShieldCheckIcon} emptyTitle={t("banned.empty")}
      columns={[
        { header: t("banned.domain"), primary: true, cell: (r) => <span className="font-mono text-sm">{r.domain}</span> },
        { header: t("banned.reason"), cell: (r) => r.reason || "-" },
        { header: t("banned.hits"), cell: (r) => r.hits },
        { header: "", cell: (r) => <Button size="icon" variant="ghost" aria-label={t("banned.remove")} onClick={() => del.mutate(r.domain)}><TrashIcon /></Button> },
      ]}
    />
    </div>
  );
}

export function AdminSecurityPage() {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("security.title")} description={t("security.hint")} />
      <PolicyCard />
      <GeoIpCard />
      <CaptchaCard />
      <DuoCard />
      <IpRulesCard />
      <TrustedProxiesCard />
      <BannedEmailsCard />
      <SecurityQuestionsAdminCard />
      <TosCard />
      <Card>
        <CardHeader><CardTitle>{t("security.api_title")}</CardTitle><CardDescription>{t("security.api_hint")}</CardDescription></CardHeader>
        <CardContent className="flex flex-wrap gap-2">
          <Button variant="outline" asChild><Link to="/admin/api-keys">{t("nav.api_keys")}</Link></Button>
          <Button variant="outline" asChild><Link to="/admin/addons">{t("nav.addons")}</Link></Button>
        </CardContent>
      </Card>
    </div>
  );
}

interface GeoIpStatus {
  loaded: boolean;
  path: string | null;
  default_path: string;
  env: string;
  attribution: string;
  attribution_label: string;
  download_hint: string;
}

/** Offline MMDB status for login-alert locations (DB-IP Lite / GeoLite2). */
function GeoIpCard() {
  const { t } = useI18n();
  const q = useQuery({ queryKey: ["admin", "security-geoip"], queryFn: () => api<GeoIpStatus>("/admin/security/geoip") });
  if (!q.data) return null;
  const d = q.data;
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("security.geoip")}</CardTitle>
        <CardDescription>{t("security.geoip_hint")}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3 text-sm">
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant={d.loaded ? "secondary" : "outline"}>{d.loaded ? t("security.geoip_loaded") : t("security.geoip_missing")}</Badge>
          <span className="font-mono text-muted-foreground break-all">{d.path ?? d.default_path}</span>
        </div>
        {!d.loaded && <p className="text-muted-foreground">{t("security.geoip_env")}</p>}
        <a className="text-primary w-fit hover:underline" href={d.attribution} target="_blank" rel="noreferrer">
          {t("security.geoip_attr")}
        </a>
      </CardContent>
    </Card>
  );
}

/** Reverse proxies / load balancers whose X-Forwarded-For header is believed. */
function TrustedProxiesCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "trusted-proxies"], queryFn: () => api<{ proxies: string[] }>("/admin/security/trusted-proxies") });
  const [text, setText] = useState("");
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => { if (q.data) setText(q.data.proxies.join("\n")); }, [q.data]);
  const save = useMutation({
    mutationFn: () => api<{ proxies: string[] }>("/admin/security/trusted-proxies", { method: "PUT", body: { proxies: text.split(/[\s,]+/).filter(Boolean) } }),
    onSuccess: (d) => { setErr(null); qc.setQueryData(["admin", "trusted-proxies"], d); void qc.invalidateQueries({ queryKey: ["admin", "ip-rules"] }); toast.success(t("common.saved")); },
    onError: (e) => setErr(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Card>
      <CardHeader><CardTitle>{t("security.trusted_proxies")}</CardTitle><CardDescription>{t("security.trusted_proxies_hint")}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-3">
        <Textarea rows={4} value={text} onChange={(e) => setText(e.target.value)} placeholder="10.0.0.0/8" aria-label={t("security.trusted_proxies")} className="font-mono" />
        <FormError message={err} />
        <div className="flex justify-end"><Button className="min-h-11 sm:min-h-9" disabled={!q.data || save.isPending} onClick={() => save.mutate()}>{t("common.save")}</Button></div>
      </CardContent>
    </Card>
  );
}
