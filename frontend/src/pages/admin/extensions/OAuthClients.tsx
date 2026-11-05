import { useState, type FormEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowsClockwiseIcon, CopyIcon, IdentificationBadgeIcon, PencilSimpleIcon, PlusIcon, TrashIcon, WarningIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { ListPage } from "@/components/ListPage";
import { ConfirmAction } from "@/components/ConfirmAction";
import { Field, FormError } from "@/components/Field";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

export interface OAuthClient { id: string; client_id: string; name: string; redirect_uris: string[]; scopes: string[]; confidential: boolean; trusted: boolean; enabled: boolean }
interface ListResp { clients: OAuthClient[]; scopes: string[]; issuer: string }
interface Draft { id?: string; name: string; uris: string; scopes: string[]; confidential: boolean; trusted: boolean; enabled: boolean }

const blank: Draft = { name: "", uris: "", scopes: ["openid", "profile", "email"], confidential: true, trusted: false, enabled: true };

/** Split the textarea into redirect URIs (one per line). */
export function parseUris(s: string): string[] {
  return s.split(/\r?\n/).map((x) => x.trim()).filter(Boolean);
}

export function AdminOAuthClientsPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["oauth-clients"], queryFn: () => api<ListResp>("/admin/oauth-clients") });
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [shown, setShown] = useState<{ client_id: string; client_secret: string | null } | null>(null);
  const fail = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const refresh = () => void qc.invalidateQueries({ queryKey: ["oauth-clients"] });
  const save = useMutation({
    mutationFn: (d: Draft) => {
      const body = { name: d.name, redirect_uris: parseUris(d.uris), scopes: d.scopes, trusted: d.trusted, enabled: d.enabled, confidential: d.confidential };
      return d.id
        ? api<OAuthClient>(`/admin/oauth-clients/${d.id}`, { method: "PATCH", body }).then(() => null)
        : api<{ client_id: string; client_secret: string | null }>("/admin/oauth-clients", { method: "POST", body });
    },
    onSuccess: (r) => { if (r) setShown(r); else toast.success(t("oauth.saved")); setDraft(null); refresh(); },
    onError: fail,
  });
  const remove = useMutation({
    mutationFn: (id: string) => api(`/admin/oauth-clients/${id}`, { method: "DELETE" }),
    onSuccess: () => { toast.success(t("oauth.deleted")); refresh(); },
  });
  const rotate = useMutation({
    mutationFn: (id: string) => api<{ client_id: string; client_secret: string }>(`/admin/oauth-clients/${id}/secret`, { method: "POST" }),
    onSuccess: (r) => { setShown(r); refresh(); },
  });
  const copy = (v: string) => { void navigator.clipboard.writeText(v); toast.success(t("apikeys.copied")); };
  const open = (d: Draft) => { setError(null); setDraft(d); };

  return (
    <ListPage<OAuthClient>
      title={t("oauth.clients_title")} description={t("oauth.clients_subtitle")}
      actions={<Button onClick={() => open(blank)}><PlusIcon aria-hidden />{t("oauth.new_client")}</Button>}
      rows={data?.clients} rowKey={(c) => c.id} loading={isLoading} emptyIcon={IdentificationBadgeIcon} emptyTitle={t("oauth.clients_empty")}
      columns={[
        { header: t("oauth.app"), primary: true, cell: (c) => <span className="flex flex-col"><span className="font-medium">{c.name}</span><code className="text-xs text-muted-foreground">{c.client_id}</code></span> },
        { header: t("oauth.type"), cell: (c) => <span className="flex flex-wrap gap-1">
          <Badge variant="outline">{c.confidential ? t("oauth.confidential") : t("oauth.public")}</Badge>
          {c.trusted && <Badge variant="secondary">{t("oauth.trusted")}</Badge>}
          {!c.enabled && <Badge variant="outline">{t("oauth.disabled")}</Badge>}
        </span> },
        { header: t("oauth.scopes"), cell: (c) => <span className="flex flex-wrap gap-1">{c.scopes.map((s) => <Badge key={s} variant="outline" className="font-mono text-[11px]">{s}</Badge>)}</span> },
        { header: "", className: "text-right", cell: (c) => (
          <span className="flex justify-end gap-1">
            <Button size="icon" variant="ghost" aria-label={t("oauth.edit")} onClick={() => open({ id: c.id, name: c.name, uris: c.redirect_uris.join("\n"), scopes: c.scopes, confidential: c.confidential, trusted: c.trusted, enabled: c.enabled })}><PencilSimpleIcon aria-hidden /></Button>
            {c.confidential && (
              <ConfirmAction description={t("oauth.confirm_rotate")} confirmLabel={t("oauth.rotate")} onConfirm={() => rotate.mutate(c.id)}>
                <Button size="icon" variant="ghost" aria-label={t("oauth.rotate")}><ArrowsClockwiseIcon aria-hidden /></Button>
              </ConfirmAction>
            )}
            <ConfirmAction description={t("oauth.confirm_delete")} confirmLabel={t("oauth.delete")} onConfirm={() => remove.mutate(c.id)}>
              <Button size="icon" variant="ghost" aria-label={t("oauth.delete")}><TrashIcon aria-hidden /></Button>
            </ConfirmAction>
          </span>
        ) },
      ]}
    >
      {data && (
        <Card>
          <CardHeader><CardTitle>{t("oauth.discovery")}</CardTitle></CardHeader>
          <CardContent className="flex gap-2">
            <Input readOnly value={`${data.issuer}/.well-known/openid-configuration`} className="font-mono text-xs" aria-label={t("oauth.discovery")} />
            <Button variant="outline" size="icon" aria-label={t("apikeys.copy")} onClick={() => copy(`${data.issuer}/.well-known/openid-configuration`)}><CopyIcon aria-hidden /></Button>
          </CardContent>
        </Card>
      )}
      {shown && (
        <Alert>
          <WarningIcon aria-hidden />
          <AlertTitle>{t("oauth.secret_once")}</AlertTitle>
          <AlertDescription className="flex flex-col gap-2">
            <Label>{t("oauth.client_id")}</Label>
            <div className="flex w-full gap-2">
              <Input readOnly value={shown.client_id} className="font-mono text-xs" aria-label={t("oauth.client_id")} />
              <Button variant="outline" size="icon" aria-label={t("apikeys.copy")} onClick={() => copy(shown.client_id)}><CopyIcon aria-hidden /></Button>
            </div>
            {shown.client_secret && (<>
              <Label>{t("oauth.client_secret")}</Label>
              <div className="flex w-full gap-2">
                <Input readOnly value={shown.client_secret} className="font-mono text-xs" aria-label={t("oauth.client_secret")} />
                <Button variant="outline" size="icon" aria-label={t("apikeys.copy")} onClick={() => copy(shown.client_secret ?? "")}><CopyIcon aria-hidden /></Button>
              </div>
            </>)}
            <Button variant="ghost" size="sm" className="self-start" onClick={() => setShown(null)}>{t("apikeys.done")}</Button>
          </AlertDescription>
        </Alert>
      )}
      <Dialog open={draft !== null} onOpenChange={(o) => !o && setDraft(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{draft?.id ? t("oauth.edit") : t("oauth.new_client")}</DialogTitle>
            <DialogDescription>{t("oauth.redirect_hint")}</DialogDescription>
          </DialogHeader>
          {draft && (
            <form className="flex flex-col gap-4" onSubmit={(e: FormEvent) => { e.preventDefault(); save.mutate(draft); }}>
              <Field id="oc-name" label={t("oauth.app")} value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} required maxLength={120} />
              <div className="flex flex-col gap-2">
                <Label htmlFor="oc-uris">{t("oauth.redirect_uris")}</Label>
                <Textarea id="oc-uris" rows={3} className="font-mono text-xs" value={draft.uris} onChange={(e) => setDraft({ ...draft, uris: e.target.value })} required />
              </div>
              <fieldset className="flex flex-col gap-2">
                <legend className="mb-2 text-sm font-medium">{t("oauth.scopes")}</legend>
                <div className="grid gap-2 rounded-md border p-3 sm:grid-cols-2">
                  {data?.scopes.map((s) => (
                    <div key={s} className="flex items-center gap-2">
                      <Checkbox id={`oc-scope-${s}`} checked={draft.scopes.includes(s)} onCheckedChange={(c) => setDraft({ ...draft, scopes: c ? [...draft.scopes, s] : draft.scopes.filter((y) => y !== s) })} />
                      <Label htmlFor={`oc-scope-${s}`} className="font-mono text-xs">{s}</Label>
                    </div>
                  ))}
                </div>
              </fieldset>
              {!draft.id && (
                <div className="flex items-center justify-between gap-4">
                  <Label htmlFor="oc-conf" className="flex flex-col items-start gap-1"><span>{t("oauth.confidential")}</span><span className="text-xs font-normal text-muted-foreground">{t("oauth.confidential_hint")}</span></Label>
                  <Switch id="oc-conf" checked={draft.confidential} onCheckedChange={(v) => setDraft({ ...draft, confidential: v })} />
                </div>
              )}
              <div className="flex items-center justify-between gap-4">
                <Label htmlFor="oc-trusted" className="flex flex-col items-start gap-1"><span>{t("oauth.trusted")}</span><span className="text-xs font-normal text-muted-foreground">{t("oauth.trusted_hint")}</span></Label>
                <Switch id="oc-trusted" checked={draft.trusted} onCheckedChange={(v) => setDraft({ ...draft, trusted: v })} />
              </div>
              {draft.id && (
                <div className="flex items-center justify-between gap-4">
                  <Label htmlFor="oc-enabled">{t("oauth.enabled")}</Label>
                  <Switch id="oc-enabled" checked={draft.enabled} onCheckedChange={(v) => setDraft({ ...draft, enabled: v })} />
                </div>
              )}
              <FormError message={error} />
              <DialogFooter><Button type="submit" disabled={save.isPending || !draft.scopes.length}>{t("oauth.save")}</Button></DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>
    </ListPage>
  );
}
