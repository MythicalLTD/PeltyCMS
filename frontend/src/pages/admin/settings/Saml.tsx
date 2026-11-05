import { useState, type FormEvent } from "react";
import { Link, useSearchParams } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowsClockwiseIcon, BuildingsIcon, CheckCircleIcon, CopyIcon, FlaskIcon, PencilSimpleIcon, PlusIcon, TrashIcon, WarningIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { ListPage } from "@/components/ListPage";
import { ConfirmAction } from "@/components/ConfirmAction";
import { Field, FormError } from "@/components/Field";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { mapToText, parseMap, splitCerts } from "@/lib/admin/saml";

interface SpUrls { entity_id: string; acs_url: string; slo_url: string; metadata_url: string; login_url: string }
interface TestResult { ok: boolean; error?: string; name_id?: string; mapped?: { email: string; name: string; groups: string[] }; attributes?: Record<string, string[]> }
export interface Idp {
  id: string; name: string; enabled: boolean; idp_entity_id: string; sso_url: string; slo_url: string; idp_certs: string[]; metadata_url: string;
  sp_entity_id: string; sign_requests: boolean; want_assertions_signed: boolean; allow_idp_initiated: boolean; clock_skew_secs: number;
  attr_email: string; attr_name: string; attr_first_name: string; attr_last_name: string; attr_groups: string;
  group_map: Record<string, string>; role_map: Record<string, string>; nameid_is_email: boolean; allow_register: boolean;
  link_by_verified_email: boolean; allow_staff: boolean; button_label: string; last_test: TestResult | null; last_test_at: string | null;
  sp: SpUrls; has_sp_key: boolean;
}
interface Draft {
  id?: string; name: string; enabled: boolean; metadata_url: string; metadata_xml: string; idp_entity_id: string; sso_url: string; slo_url: string; certs: string;
  sp_entity_id: string; sign_requests: boolean; want_assertions_signed: boolean; allow_idp_initiated: boolean; clock_skew_secs: number;
  attr_email: string; attr_name: string; attr_first_name: string; attr_last_name: string; attr_groups: string; group_map: string; role_map: string;
  nameid_is_email: boolean; allow_register: boolean; link_by_verified_email: boolean; allow_staff: boolean; button_label: string;
}

const blank: Draft = {
  name: "", enabled: true, metadata_url: "", metadata_xml: "", idp_entity_id: "", sso_url: "", slo_url: "", certs: "", sp_entity_id: "",
  sign_requests: false, want_assertions_signed: true, allow_idp_initiated: false, clock_skew_secs: 120,
  attr_email: "email", attr_name: "displayName", attr_first_name: "givenName", attr_last_name: "sn", attr_groups: "groups", group_map: "", role_map: "",
  nameid_is_email: false, allow_register: false, link_by_verified_email: false, allow_staff: false, button_label: "",
};

const toDraft = (i: Idp): Draft => ({
  ...blank, id: i.id, name: i.name, enabled: i.enabled, metadata_url: i.metadata_url, idp_entity_id: i.idp_entity_id, sso_url: i.sso_url, slo_url: i.slo_url,
  certs: i.idp_certs.join("\n"), sp_entity_id: i.sp_entity_id, sign_requests: i.sign_requests, want_assertions_signed: i.want_assertions_signed,
  allow_idp_initiated: i.allow_idp_initiated, clock_skew_secs: i.clock_skew_secs, attr_email: i.attr_email, attr_name: i.attr_name,
  attr_first_name: i.attr_first_name, attr_last_name: i.attr_last_name, attr_groups: i.attr_groups, group_map: mapToText(i.group_map), role_map: mapToText(i.role_map),
  nameid_is_email: i.nameid_is_email, allow_register: i.allow_register, link_by_verified_email: i.link_by_verified_email, allow_staff: i.allow_staff, button_label: i.button_label,
});

function Toggle({ id, label, hint, checked, onChange }: { id: string; label: string; hint?: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <Label htmlFor={id} className="flex flex-col items-start gap-1"><span>{label}</span>{hint && <span className="text-xs font-normal text-muted-foreground">{hint}</span>}</Label>
      <Switch id={id} checked={checked} onCheckedChange={onChange} />
    </div>
  );
}

function Result({ r }: { r: TestResult }) {
  const { t, tCode } = useI18n();
  if (!r.ok) return <Alert variant="destructive"><WarningIcon aria-hidden /><AlertTitle>{t("saml.test_failed")}</AlertTitle><AlertDescription><code className="text-xs">{r.error ?? ""}</code> {tCode(`error.saml_reason.${(r.error ?? "").split(".")[0]}`)}</AlertDescription></Alert>;
  return (
    <Alert><CheckCircleIcon aria-hidden /><AlertTitle>{t("saml.test_ok")}</AlertTitle>
      <AlertDescription className="flex flex-col gap-1 text-xs">
        <span>NameID: <code>{r.name_id}</code></span>
        <span>{t("saml.attr_email")}: <code>{r.mapped?.email || "-"}</code> · {t("saml.attr_name")}: <code>{r.mapped?.name || "-"}</code></span>
        <span>{t("saml.attr_groups")}: {(r.mapped?.groups ?? []).map((g) => <Badge key={g} variant="outline" className="mr-1">{g}</Badge>)}</span>
      </AlertDescription>
    </Alert>
  );
}

export function AdminSamlPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [params] = useSearchParams();
  const tested = params.get("tested");
  const { data, isLoading } = useQuery({ queryKey: ["saml-idps"], queryFn: () => api<{ addon_enabled: boolean; idps: Idp[] }>("/admin/saml/idps") });
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [validating, setValidating] = useState<Idp | null>(null);
  const [paste, setPaste] = useState("");
  const [dry, setDry] = useState<TestResult | null>(null);
  const fail = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const refresh = () => void qc.invalidateQueries({ queryKey: ["saml-idps"] });
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setDraft((d) => (d ? { ...d, [k]: v } : d));

  const save = useMutation({
    mutationFn: (d: Draft) => {
      const body = {
        name: d.name, enabled: d.enabled, metadata_url: d.metadata_url, metadata_xml: d.metadata_xml, idp_entity_id: d.idp_entity_id, sso_url: d.sso_url, slo_url: d.slo_url,
        idp_certs: splitCerts(d.certs), sp_entity_id: d.sp_entity_id, sign_requests: d.sign_requests, want_assertions_signed: d.want_assertions_signed,
        allow_idp_initiated: d.allow_idp_initiated, clock_skew_secs: Number(d.clock_skew_secs) || 0, attr_email: d.attr_email, attr_name: d.attr_name,
        attr_first_name: d.attr_first_name, attr_last_name: d.attr_last_name, attr_groups: d.attr_groups, group_map: parseMap(d.group_map), role_map: parseMap(d.role_map),
        nameid_is_email: d.nameid_is_email, allow_register: d.allow_register, link_by_verified_email: d.link_by_verified_email, allow_staff: d.allow_staff, button_label: d.button_label,
      };
      return d.id ? api<Idp>(`/admin/saml/idps/${d.id}`, { method: "PUT", body }) : api<Idp>("/admin/saml/idps", { method: "POST", body });
    },
    onSuccess: () => { toast.success(t("saml.saved")); setDraft(null); refresh(); },
    onError: fail,
  });
  const importMd = useMutation({
    mutationFn: (d: Draft) => api<{ entity_id: string; sso_url: string; slo_url: string; certs: string[] }>("/admin/saml/metadata", { method: "POST", body: { xml: d.metadata_xml, url: d.metadata_url } }),
    onSuccess: (m) => { setDraft((d) => (d ? { ...d, idp_entity_id: m.entity_id, sso_url: m.sso_url, slo_url: m.slo_url, certs: m.certs.join("\n") } : d)); toast.success(t("saml.metadata_loaded")); },
    onError: fail,
  });
  const remove = useMutation({ mutationFn: (id: string) => api(`/admin/saml/idps/${id}`, { method: "DELETE" }), onSuccess: () => { toast.success(t("saml.deleted")); refresh(); } });
  const rotate = useMutation({ mutationFn: (id: string) => api(`/admin/saml/idps/${id}/rotate-key`, { method: "POST" }), onSuccess: () => { toast.success(t("saml.rotated")); refresh(); } });
  const validate = useMutation({
    mutationFn: ({ id, xml }: { id: string; xml: string }) => api<TestResult>(`/admin/saml/idps/${id}/validate`, { method: "POST", body: { saml_response: xml } }),
    onSuccess: setDry,
  });
  const copy = (v: string) => { void navigator.clipboard.writeText(v); toast.success(t("apikeys.copied")); };
  const open = (d: Draft) => { setError(null); setDraft(d); };
  const testedIdp = data?.idps.find((i) => i.id === tested);

  return (
    <ListPage<Idp>
      title={t("saml.title")} description={t("saml.subtitle")} actions={<Button onClick={() => open(blank)}><PlusIcon aria-hidden />{t("saml.new")}</Button>}
      rows={data?.idps} rowKey={(i) => i.id} loading={isLoading} emptyIcon={BuildingsIcon} emptyTitle={t("saml.empty")}
      columns={[
        { header: t("saml.name"), primary: true, cell: (i) => <span className="flex min-w-0 flex-col"><span className="font-medium">{i.name}</span><code className="truncate text-xs text-muted-foreground">{i.idp_entity_id}</code></span> },
        { header: t("saml.status"), cell: (i) => <span className="flex flex-wrap gap-1">
          <Badge variant={i.enabled ? "secondary" : "outline"}>{i.enabled ? t("saml.enabled") : t("saml.disabled")}</Badge>
          {i.allow_register && <Badge variant="outline">{t("saml.auto_provision")}</Badge>}
          {i.sign_requests && <Badge variant="outline">{t("saml.signed_requests")}</Badge>}
          {i.last_test && <Badge variant={i.last_test.ok ? "secondary" : "destructive"}>{i.last_test.ok ? t("saml.test_ok") : t("saml.test_failed")}{i.last_test_at ? ` · ${formatDateTime(i.last_test_at, lang)}` : ""}</Badge>}
        </span> },
        { header: t("saml.sp_metadata"), cell: (i) => (
          <span className="flex items-center gap-1">
            <code className="max-w-56 truncate text-xs">{i.sp.metadata_url}</code>
            <Button size="icon" variant="ghost" aria-label={t("apikeys.copy")} onClick={() => copy(i.sp.metadata_url)}><CopyIcon aria-hidden /></Button>
          </span>
        ) },
        { header: "", className: "text-right", cell: (i) => (
          <span className="flex justify-end gap-1">
            <Button size="icon" variant="ghost" aria-label={t("saml.test")} asChild><a href={`${i.sp.login_url}?mode=test`}><FlaskIcon aria-hidden /></a></Button>
            <Button size="sm" variant="ghost" onClick={() => { setDry(null); setPaste(""); setValidating(i); }}>{t("saml.validate")}</Button>
            <Button size="icon" variant="ghost" aria-label={t("saml.edit")} onClick={() => open(toDraft(i))}><PencilSimpleIcon aria-hidden /></Button>
            {i.sign_requests && (
              <ConfirmAction description={t("saml.confirm_rotate")} confirmLabel={t("saml.rotate")} onConfirm={() => rotate.mutate(i.id)}>
                <Button size="icon" variant="ghost" aria-label={t("saml.rotate")}><ArrowsClockwiseIcon aria-hidden /></Button>
              </ConfirmAction>
            )}
            <ConfirmAction description={t("saml.confirm_delete")} confirmLabel={t("saml.delete")} onConfirm={() => remove.mutate(i.id)}>
              <Button size="icon" variant="ghost" aria-label={t("saml.delete")}><TrashIcon aria-hidden /></Button>
            </ConfirmAction>
          </span>
        ) },
      ]}
    >
      {data && !data.addon_enabled && <Alert><WarningIcon aria-hidden /><AlertTitle>{t("saml.addon_off")}</AlertTitle><AlertDescription className="flex flex-col gap-2"><span>{t("saml.addon_off_hint")}</span><Button asChild size="sm" variant="outline" className="self-start"><Link to="/admin/addons">{t("saml.enable_addon")}</Link></Button></AlertDescription></Alert>}
      {testedIdp?.last_test && <Result r={testedIdp.last_test} />}
      <Dialog open={draft !== null} onOpenChange={(o) => !o && setDraft(null)}>
        <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{draft?.id ? t("saml.edit") : t("saml.new")}</DialogTitle>
            <DialogDescription>{t("saml.form_hint")}</DialogDescription>
          </DialogHeader>
          {draft && (
            <form className="flex flex-col gap-5" onSubmit={(e: FormEvent) => { e.preventDefault(); save.mutate(draft); }}>
              <div className="grid gap-4 sm:grid-cols-2">
                <Field id="sa-name" label={t("saml.name")} value={draft.name} onChange={(e) => set("name", e.target.value)} required maxLength={100} />
                <Field id="sa-label" label={t("saml.button_label")} value={draft.button_label} onChange={(e) => set("button_label", e.target.value)} maxLength={60} />
              </div>
              <Card>
                <CardHeader><CardTitle className="text-base">{t("saml.idp_section")}</CardTitle><CardDescription>{t("saml.metadata_hint")}</CardDescription></CardHeader>
                <CardContent className="flex flex-col gap-4">
                  <Field id="sa-mdurl" label={t("saml.metadata_url")} type="url" value={draft.metadata_url} onChange={(e) => set("metadata_url", e.target.value)} placeholder="https://idp.example.com/metadata" />
                  <div className="flex flex-col gap-2">
                    <Label htmlFor="sa-mdxml">{t("saml.metadata_xml")}</Label>
                    <Textarea id="sa-mdxml" rows={3} className="font-mono text-xs" value={draft.metadata_xml} onChange={(e) => set("metadata_xml", e.target.value)} />
                  </div>
                  <Button type="button" variant="outline" className="self-start" disabled={importMd.isPending || (!draft.metadata_xml.trim() && !draft.metadata_url.trim())} onClick={() => importMd.mutate(draft)}>{t("saml.load_metadata")}</Button>
                  <Field id="sa-ent" label={t("saml.idp_entity_id")} value={draft.idp_entity_id} onChange={(e) => set("idp_entity_id", e.target.value)} />
                  <div className="grid gap-4 sm:grid-cols-2">
                    <Field id="sa-sso" label={t("saml.sso_url")} type="url" value={draft.sso_url} onChange={(e) => set("sso_url", e.target.value)} />
                    <Field id="sa-slo" label={t("saml.slo_url")} type="url" value={draft.slo_url} onChange={(e) => set("slo_url", e.target.value)} />
                  </div>
                  <div className="flex flex-col gap-2">
                    <Label htmlFor="sa-cert">{t("saml.certs")}</Label>
                    <Textarea id="sa-cert" rows={4} className="font-mono text-xs" value={draft.certs} onChange={(e) => set("certs", e.target.value)} placeholder="-----BEGIN CERTIFICATE-----" />
                  </div>
                </CardContent>
              </Card>
              <Card>
                <CardHeader><CardTitle className="text-base">{t("saml.mapping")}</CardTitle><CardDescription>{t("saml.mapping_hint")}</CardDescription></CardHeader>
                <CardContent className="flex flex-col gap-4">
                  <div className="grid gap-4 sm:grid-cols-2">
                    <Field id="sa-ae" label={t("saml.attr_email")} value={draft.attr_email} onChange={(e) => set("attr_email", e.target.value)} />
                    <Field id="sa-an" label={t("saml.attr_name")} value={draft.attr_name} onChange={(e) => set("attr_name", e.target.value)} />
                    <Field id="sa-af" label={t("saml.attr_first_name")} value={draft.attr_first_name} onChange={(e) => set("attr_first_name", e.target.value)} />
                    <Field id="sa-al" label={t("saml.attr_last_name")} value={draft.attr_last_name} onChange={(e) => set("attr_last_name", e.target.value)} />
                    <Field id="sa-ag" label={t("saml.attr_groups")} value={draft.attr_groups} onChange={(e) => set("attr_groups", e.target.value)} />
                  </div>
                  <div className="grid gap-4 sm:grid-cols-2">
                    <div className="flex flex-col gap-2">
                      <Label htmlFor="sa-gm">{t("saml.group_map")}</Label>
                      <Textarea id="sa-gm" rows={3} className="font-mono text-xs" value={draft.group_map} onChange={(e) => set("group_map", e.target.value)} placeholder="vip = <client-group-id>" />
                    </div>
                    <div className="flex flex-col gap-2">
                      <Label htmlFor="sa-rm">{t("saml.role_map")}</Label>
                      <Textarea id="sa-rm" rows={3} className="font-mono text-xs" value={draft.role_map} onChange={(e) => set("role_map", e.target.value)} placeholder="it-admins = support" />
                    </div>
                  </div>
                  <Toggle id="sa-nie" label={t("saml.nameid_is_email")} checked={draft.nameid_is_email} onChange={(v) => set("nameid_is_email", v)} />
                </CardContent>
              </Card>
              <Card>
                <CardHeader><CardTitle className="text-base">{t("saml.security")}</CardTitle></CardHeader>
                <CardContent className="flex flex-col gap-4">
                  <Toggle id="sa-en" label={t("saml.enabled")} checked={draft.enabled} onChange={(v) => set("enabled", v)} />
                  <Toggle id="sa-reg" label={t("saml.auto_provision")} hint={t("saml.auto_provision_hint")} checked={draft.allow_register} onChange={(v) => set("allow_register", v)} />
                  <Toggle id="sa-link" label={t("saml.link_email")} hint={t("saml.link_email_hint")} checked={draft.link_by_verified_email} onChange={(v) => set("link_by_verified_email", v)} />
                  <Toggle id="sa-staff" label={t("saml.allow_staff")} hint={t("saml.allow_staff_hint")} checked={draft.allow_staff} onChange={(v) => set("allow_staff", v)} />
                  <Toggle id="sa-sign" label={t("saml.signed_requests")} checked={draft.sign_requests} onChange={(v) => set("sign_requests", v)} />
                  <Toggle id="sa-was" label={t("saml.want_signed")} hint={t("saml.want_signed_hint")} checked={draft.want_assertions_signed} onChange={(v) => set("want_assertions_signed", v)} />
                  <Toggle id="sa-idpi" label={t("saml.idp_initiated")} hint={t("saml.idp_initiated_hint")} checked={draft.allow_idp_initiated} onChange={(v) => set("allow_idp_initiated", v)} />
                  <div className="grid gap-4 sm:grid-cols-2">
                    <Field id="sa-skew" label={t("saml.skew")} type="number" min={0} max={600} value={String(draft.clock_skew_secs)} onChange={(e) => set("clock_skew_secs", Number(e.target.value))} />
                    <Field id="sa-spent" label={t("saml.sp_entity_id")} value={draft.sp_entity_id} onChange={(e) => set("sp_entity_id", e.target.value)} placeholder={t("saml.sp_entity_default")} />
                  </div>
                </CardContent>
              </Card>
              <FormError message={error} />
              <DialogFooter><Button type="submit" disabled={save.isPending}>{t("saml.save")}</Button></DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>

      <Dialog open={validating !== null} onOpenChange={(o) => !o && setValidating(null)}>
        <DialogContent className="sm:max-w-xl">
          <DialogHeader><DialogTitle>{t("saml.validate")}</DialogTitle><DialogDescription>{t("saml.validate_hint")}</DialogDescription></DialogHeader>
          {validating && (
            <div className="flex flex-col gap-3">
              <div className="grid gap-2 text-xs">
                {(["entity_id", "acs_url", "slo_url", "metadata_url"] as const).map((k) => (
                  <div key={k} className="flex items-center gap-2"><Input readOnly value={validating.sp[k]} className="font-mono text-xs" aria-label={k} /><Button variant="outline" size="icon" aria-label={t("apikeys.copy")} onClick={() => copy(validating.sp[k])}><CopyIcon aria-hidden /></Button></div>
                ))}
              </div>
              <Textarea rows={6} className="font-mono text-xs" value={paste} onChange={(e) => setPaste(e.target.value)} aria-label="SAMLResponse" placeholder="SAMLResponse (base64) / <samlp:Response …>" />
              <Button className="self-start" disabled={!paste.trim() || validate.isPending} onClick={() => validate.mutate({ id: validating.id, xml: paste })}>{t("saml.validate")}</Button>
              {dry && <Result r={dry} />}
            </div>
          )}
        </DialogContent>
      </Dialog>
    </ListPage>
  );
}
