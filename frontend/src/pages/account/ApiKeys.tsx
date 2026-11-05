import { useState, type FormEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CopyIcon, GlobeIcon, KeyIcon, PlusIcon, ProhibitIcon, WarningIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { ListCard } from "@/components/ListPage";
import { Field, FormError } from "@/components/Field";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { ConfirmAction } from "@/components/ConfirmAction";

interface ApiKey { id: string; name: string; prefix: string; scopes: string[]; allowed_ips: string[]; expires_at: string | null; last_used_at: string | null; revoked_at: string | null; created_at: string }

/** Split a textarea of IPs / CIDR ranges (newline, comma or space separated). */
export function parseIpList(text: string): string[] {
  return text.split(/[\s,]+/).map((x) => x.trim()).filter(Boolean);
}

/** Allowed-IP badges ("any IP" when empty). */
export function IpBadges({ ips }: { ips: string[] }) {
  const { t } = useI18n();
  if (!ips.length) return <span className="text-sm text-muted-foreground">{t("apikeys.any_ip")}</span>;
  return <span className="flex flex-wrap gap-1">{ips.map((ip) => <Badge key={ip} variant="outline" className="font-mono text-[11px]">{ip}</Badge>)}</span>;
}

/** Edit a live key's IP allowlist (`base` = "/api-keys" or "/admin/api-keys"). */
export function KeyIpButton({ k, base, onSaved }: { k: { id: string; allowed_ips: string[] }; base: string; onSaved: () => void }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: () => api(`${base}/${k.id}`, { method: "PATCH", body: { allowed_ips: parseIpList(text) } }),
    onSuccess: () => { toast.success(t("apikeys.ips_saved")); setOpen(false); onSaved(); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <>
      <Button size="sm" variant="ghost" onClick={() => { setText(k.allowed_ips.join("\n")); setError(null); setOpen(true); }}><GlobeIcon aria-hidden />{t("apikeys.edit_ips")}</Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("apikeys.allowed_ips")}</DialogTitle>
            <DialogDescription>{t("apikeys.allowed_ips_hint")}</DialogDescription>
          </DialogHeader>
          <form className="flex flex-col gap-4" onSubmit={(e: FormEvent) => { e.preventDefault(); save.mutate(); }}>
            <Textarea aria-label={t("apikeys.allowed_ips")} className="font-mono text-xs" rows={5} value={text} onChange={(e) => setText(e.target.value)} placeholder="203.0.113.7&#10;10.0.0.0/8" />
            <FormError message={error} />
            <DialogFooter><Button type="submit" disabled={save.isPending}>{t("apikeys.save")}</Button></DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}

export function ApiKeysPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["api-keys"], queryFn: () => api<{ items: ApiKey[]; available_scopes: string[] }>("/api-keys") });
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [days, setDays] = useState("90");
  const [scopes, setScopes] = useState<string[]>([]);
  const [ips, setIps] = useState("");
  const [secret, setSecret] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const fail = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const create = useMutation({
    mutationFn: () => api<{ secret: string }>("/api-keys", { method: "POST", body: { name, scopes, expires_in_days: days ? Number(days) : null, allowed_ips: parseIpList(ips) } }),
    onSuccess: (r) => { setSecret(r.secret); setOpen(false); setName(""); setScopes([]); setIps(""); void qc.invalidateQueries({ queryKey: ["api-keys"] }); },
    onError: fail,
  });
  const revoke = useMutation({
    mutationFn: (id: string) => api(`/api-keys/${id}`, { method: "DELETE" }),
    onSuccess: () => { toast.success(t("apikeys.revoked")); void qc.invalidateQueries({ queryKey: ["api-keys"] }); },
  });
  const status = (k: ApiKey) => k.revoked_at ? <Badge variant="outline">{t("apikeys.status_revoked")}</Badge>
    : k.expires_at && new Date(k.expires_at) < new Date() ? <Badge variant="outline">{t("apikeys.status_expired")}</Badge>
    : <Badge variant="secondary">{t("apikeys.status_active")}</Badge>;
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("apikeys.title")} description={t("apikeys.subtitle")}
        actions={<Button onClick={() => { setError(null); setOpen(true); }}><PlusIcon aria-hidden />{t("apikeys.new")}</Button>} />
      {secret && (
        <Alert>
          <WarningIcon aria-hidden />
          <AlertTitle>{t("apikeys.copy_now")}</AlertTitle>
          <AlertDescription className="flex flex-col gap-2">
            <div className="flex w-full gap-2">
              <Input readOnly value={secret} className="font-mono text-xs" aria-label={t("apikeys.secret")} onFocus={(e) => e.currentTarget.select()} />
              <Button variant="outline" size="icon" aria-label={t("apikeys.copy")} onClick={() => { void navigator.clipboard.writeText(secret); toast.success(t("apikeys.copied")); }}><CopyIcon aria-hidden /></Button>
            </div>
            <Button variant="ghost" size="sm" className="self-start" onClick={() => setSecret(null)}>{t("apikeys.done")}</Button>
          </AlertDescription>
        </Alert>
      )}
      <ListCard<ApiKey>
        rows={data?.items} rowKey={(k) => k.id} loading={isLoading} emptyIcon={KeyIcon} emptyTitle={t("apikeys.empty")}
        columns={[
          { header: t("apikeys.name"), cell: (k) => <span className="flex flex-col"><span className="font-medium">{k.name}</span><code className="text-xs text-muted-foreground">{k.prefix}…</code></span>, primary: true },
          { header: t("apikeys.scopes"), cell: (k) => <span className="flex flex-wrap gap-1">{k.scopes.map((s) => <Badge key={s} variant="outline" className="font-mono text-[11px]">{s}</Badge>)}</span> },
          { header: t("apikeys.allowed_ips"), cell: (k) => <IpBadges ips={k.allowed_ips ?? []} /> },
          { header: t("apikeys.status"), cell: status },
          { header: t("apikeys.last_used"), cell: (k) => (k.last_used_at ? formatDateTime(k.last_used_at, lang) : t("apikeys.never")) },
          { header: t("apikeys.expires"), cell: (k) => (k.expires_at ? formatDateTime(k.expires_at, lang) : t("apikeys.never")) },
          { header: "", className: "text-right", cell: (k) => !k.revoked_at && (
            <span className="flex flex-wrap justify-end gap-1">
              <KeyIpButton k={{ id: k.id, allowed_ips: k.allowed_ips ?? [] }} base="/api-keys" onSaved={() => void qc.invalidateQueries({ queryKey: ["api-keys"] })} />
              <ConfirmAction description={t("apikeys.confirm_revoke")} confirmLabel={t("apikeys.revoke")} onConfirm={() => revoke.mutate(k.id)}><Button size="sm" variant="ghost"><ProhibitIcon aria-hidden />{t("apikeys.revoke")}</Button></ConfirmAction>
            </span>
          ) },
        ]}
      />
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("apikeys.new")}</DialogTitle>
            <DialogDescription>{t("apikeys.scope_hint")}</DialogDescription>
          </DialogHeader>
          <form className="flex flex-col gap-4" onSubmit={(e: FormEvent) => { e.preventDefault(); create.mutate(); }}>
            <Field id="key-name" label={t("apikeys.name")} value={name} onChange={(e) => setName(e.target.value)} required maxLength={80} />
            <Field id="key-days" label={t("apikeys.expires_days")} hint={t("apikeys.expires_hint")} inputMode="numeric" value={days} onChange={(e) => setDays(e.target.value.replace(/\D/g, ""))} />
            <div className="flex flex-col gap-2">
              <Label htmlFor="key-ips">{t("apikeys.allowed_ips")}</Label>
              <Textarea id="key-ips" className="font-mono text-xs" rows={2} value={ips} onChange={(e) => setIps(e.target.value)} placeholder="203.0.113.7, 10.0.0.0/8" />
              <p className="text-xs text-muted-foreground">{t("apikeys.allowed_ips_hint")}</p>
            </div>
            <fieldset className="flex flex-col gap-2">
              <legend className="mb-2 text-sm font-medium">{t("apikeys.scopes")}</legend>
              <div className="grid max-h-56 gap-2 overflow-y-auto rounded-md border p-3 sm:grid-cols-2">
                {data?.available_scopes.map((s) => (
                  <div key={s} className="flex items-center gap-2">
                    <Checkbox id={`scope-${s}`} checked={scopes.includes(s)} onCheckedChange={(c) => setScopes((x) => (c ? [...x, s] : x.filter((y) => y !== s)))} />
                    <Label htmlFor={`scope-${s}`} className="font-mono text-xs">{s}</Label>
                  </div>
                ))}
              </div>
            </fieldset>
            <FormError message={error} />
            <DialogFooter><Button type="submit" disabled={create.isPending || !scopes.length}>{t("apikeys.create")}</Button></DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  );
}
