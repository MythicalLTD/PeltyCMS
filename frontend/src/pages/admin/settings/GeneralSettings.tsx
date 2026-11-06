import { useState } from "react";
import { Replaceable } from "@/components/plugin-ui/AddonSlots";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRightIcon, WarningIcon } from "@phosphor-icons/react";
import { Link } from "react-router";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { ImageUrlField } from "@/components/admin/ImageUrlField";
import { api, ApiError } from "@/lib/api";
import { httpsOrEmpty, logoUrlOk, type CustomCode, type GeneralSettings } from "@/lib/admin/general";
import { useI18n } from "@/lib/i18n";

type Section = keyof GeneralSettings;
type Kind = "text" | "url" | "email" | "number" | "bool" | "textarea" | { select: string[] };
/** Declarative field list per tab; labels are `general.f.<section>.<key>`. */
const FIELDS: Record<Section, [string, Kind][]> = {
  general: [["company_name", "text"], ["email", "email"], ["domain", "url"], ["logo_url", "url"], ["pay_to_text", "textarea"], ["records_per_page", "number"]],
  system_urls: [["system_url", "url"], ["terms_url", "url"], ["privacy_url", "url"], ["support_url", "url"]],
  maintenance: [["enabled", "bool"], ["message", "textarea"], ["redirect_url", "url"]],
  localisation: [["default_language", { select: ["en", "ro"] }], ["allow_language_choice", "bool"], ["date_format", { select: [] }], ["client_date_format", { select: [] }], ["timezone", "text"], ["default_country", "text"]],
  ordering: [["default_order_form", { select: [] }], ["tos_required", "bool"], ["allow_domain_only", "bool"], ["show_cart_after_add", "bool"], ["enable_promo_field", "bool"], ["grace_days", "number"]],
  support: [["attachment_max_mb", "number"], ["attachments_per_message", "number"], ["live_chat", "bool"]],
};

export function AdminGeneralSettingsPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "general-settings"], queryFn: () => api<{ settings: GeneralSettings; date_formats: string[]; order_forms: string[] }>("/admin/general-settings") });
  const [s, setS] = useState<GeneralSettings | null>(q.data?.settings ?? null);
  const [seen, setSeen] = useState(q.data);
  if (q.data !== seen) {
    setSeen(q.data);
    if (q.data !== undefined) setS(q.data.settings);
  }
  const save = useMutation({
    mutationFn: (b: GeneralSettings) => api("/admin/general-settings", { method: "PUT", body: b }),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ["admin", "general-settings"] }); qc.invalidateQueries({ queryKey: ["public", "general"] }); toast.success(t("common.saved")); },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  });
  if (!s || !q.data) return <Skeleton className="h-64" />;
  const opts = (key: string, k: { select: string[] }) => k.select.length ? k.select : key === "default_order_form" ? q.data!.order_forms : q.data!.date_formats;
  const set = (sec: Section, key: string, v: unknown) => setS({ ...s, [sec]: { ...s[sec], [key]: v } } as GeneralSettings);
  const invalid = (Object.keys(FIELDS) as Section[]).some((sec) =>
    FIELDS[sec].some(([k, kind]) => {
      if (kind !== "url") return false;
      const v = String((s[sec] as Record<string, unknown>)[k] ?? "");
      return k === "logo_url" ? !logoUrlOk(v) : !httpsOrEmpty(v);
    }),
  );
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("general.title")} description={t("general.description")} actions={<Button disabled={invalid || save.isPending} onClick={() => save.mutate(s)}>{t("common.save")}</Button>} />
      <Tabs defaultValue="general">
        <div className="overflow-x-auto"><TabsList>
          {(Object.keys(FIELDS) as Section[]).map((sec) => <TabsTrigger key={sec} value={sec}>{tCode(`general.tab.${sec}`)}</TabsTrigger>)}
          <TabsTrigger value="custom_code">{t("general.tab.custom_code")}</TabsTrigger>
          <TabsTrigger value="security">{t("general.tab.security")}</TabsTrigger>
          <TabsTrigger value="social">{t("general.tab.social")}</TabsTrigger>
        </TabsList></div>
        {(Object.keys(FIELDS) as Section[]).map((sec) => (
          <TabsContent key={sec} value={sec} className="pt-2">
            <Card><CardContent className="grid gap-4 pt-6 md:grid-cols-2">
              {FIELDS[sec].map(([key, kind]) => {
                const id = `${sec}-${key}`;
                const v = (s[sec] as Record<string, unknown>)[key];
                const label = tCode(`general.f.${sec}.${key}`);
                if (kind === "bool") return <div key={key} className="flex items-center justify-between gap-4 rounded-md border p-3 md:col-span-2"><Label htmlFor={id}>{label}</Label><Switch id={id} checked={Boolean(v)} onCheckedChange={(x) => set(sec, key, x)} /></div>;
                if (key === "logo_url") {
                  return (
                    <ImageUrlField
                      key={key}
                      id={id}
                      className="md:col-span-2"
                      label={label}
                      value={String(v ?? "")}
                      onChange={(url) => set(sec, key, url)}
                      preferPath
                      hint={t("media.hint")}
                    />
                  );
                }
                return (
                  <div key={key} className={`flex flex-col gap-2 ${kind === "textarea" ? "md:col-span-2" : ""}`}>
                    <Label htmlFor={id}>{label}</Label>
                    {typeof kind === "object" ? (
                      <Select value={String(v)} onValueChange={(x) => set(sec, key, x)}><SelectTrigger id={id} className="w-full"><SelectValue /></SelectTrigger><SelectContent>{opts(key, kind).map((o) => <SelectItem key={o} value={o}>{o}</SelectItem>)}</SelectContent></Select>
                    ) : kind === "textarea" ? <Textarea id={id} rows={3} value={String(v ?? "")} onChange={(e) => set(sec, key, e.target.value)} />
                      : <Input id={id} type={kind === "number" ? "number" : kind === "email" ? "email" : kind === "url" ? "url" : "text"} inputMode={kind === "number" ? "numeric" : undefined} placeholder={kind === "url" ? "https://" : undefined}
                          aria-invalid={kind === "url" && !httpsOrEmpty(String(v ?? ""))} value={String(v ?? "")}
                          onChange={(e) => set(sec, key, kind === "number" ? Math.max(0, Math.trunc(Number(e.target.value) || 0)) : e.target.value)} />}
                  </div>
                );
              })}
            </CardContent></Card>
          </TabsContent>
        ))}
        <TabsContent value="custom_code" className="pt-2"><Replaceable section="tab-custom_code"><CustomCodeCard /></Replaceable></TabsContent>
        <TabsContent value="security" className="pt-2"><Replaceable section="tab-security"><LinksCard links={SECURITY_LINKS} /></Replaceable></TabsContent>
        <TabsContent value="social" className="pt-2"><Replaceable section="tab-social"><LinksCard links={SOCIAL_LINKS} /></Replaceable></TabsContent>
      </Tabs>
    </div>
  );
}

function CustomCodeCard() {
  const { t } = useI18n();
  const q = useQuery({ queryKey: ["admin", "custom-code"], queryFn: () => api<CustomCode>("/admin/custom-code"), retry: false });
  const [c, setC] = useState<CustomCode>(q.data ?? { css: "", head_js: "", footer_js: "" });
  const [seen, setSeen] = useState(q.data);
  if (q.data !== seen) {
    setSeen(q.data);
    if (q.data !== undefined) setC(q.data);
  }
  const save = useMutation({
    mutationFn: () => api("/admin/custom-code", { method: "PUT", body: c }),
    onSuccess: () => toast.success(t("common.saved")),
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  });
  if (q.error) return <Alert><WarningIcon /><AlertTitle>{t("general.code_admin_only")}</AlertTitle></Alert>;
  return (
    <Card>
      <CardHeader><CardTitle>{t("general.tab.custom_code")}</CardTitle><CardDescription>{t("general.code_hint")}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-4">
        <Alert variant="destructive"><WarningIcon /><AlertTitle>{t("general.code_warning_title")}</AlertTitle><AlertDescription>{t("general.code_warning")}</AlertDescription></Alert>
        {(["css", "head_js", "footer_js"] as const).map((k) => (
          <div key={k} className="flex flex-col gap-2">
            <Label htmlFor={`code-${k}`}>{t(`general.code.${k}`)}</Label>
            <Textarea id={`code-${k}`} rows={6} className="font-mono text-xs" spellCheck={false} value={c[k]} onChange={(e) => setC({ ...c, [k]: e.target.value })} />
          </div>
        ))}
        <Button className="self-start" disabled={save.isPending} onClick={() => save.mutate()}>{t("common.save")}</Button>
      </CardContent>
    </Card>
  );
}

/** Security / social settings live on their own pages; this tab only links there. */
export const SECURITY_LINKS = [
  { to: "/admin/fraud", key: "fraud" },
  { to: "/admin/sessions", key: "sessions" },
  { to: "/admin/api-keys", key: "api_keys" },
  { to: "/admin/audit", key: "audit" },
  { to: "/admin/gdpr", key: "gdpr" },
] as const;
export const SOCIAL_LINKS = [{ to: "/admin/website", key: "social" }] as const;

function LinksCard({ links }: { links: readonly { to: string; key: string }[] }) {
  const { t, tCode } = useI18n();
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      {links.map((l) => (
        <Card key={l.to}>
          <CardHeader><CardTitle>{tCode(`general.link.${l.key}`)}</CardTitle><CardDescription>{tCode(`general.link.${l.key}_help`)}</CardDescription></CardHeader>
          <CardContent><Button asChild variant="outline" size="sm"><Link to={l.to}>{t("general.link.open")}<ArrowRightIcon aria-hidden /></Link></Button></CardContent>
        </Card>
      ))}
    </div>
  );
}
