import { useState } from "react";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CaretLeftIcon, CaretRightIcon, KeyIcon, ProhibitIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { ApiKeysPage, IpBadges, KeyIpButton } from "@/pages/account/ApiKeys";
import { ConfirmAction } from "@/components/ConfirmAction";
import { ListCard } from "@/components/ListPage";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

interface OwnedKey { id: string; name: string; prefix: string; scopes: string[]; allowed_ips: string[]; expires_at: string | null; last_used_at: string | null; revoked_at: string | null; created_at: string; user_id: string; user_email: string }
interface PageOf<T> { items: T[]; page: number; per_page: number; total: number }

export function AllApiKeysCard() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [q, setQ] = useState("");
  const [active, setActive] = useState(true);
  const [page, setPage] = useState(1);
  const params = new URLSearchParams({ page: String(page), per_page: "25", active: String(active), ...(q.trim() ? { q: q.trim() } : {}) });
  const { data, isLoading } = useQuery({ queryKey: ["admin", "api-keys", page, active, q], queryFn: () => api<PageOf<OwnedKey>>(`/admin/api-keys?${params}`), placeholderData: keepPreviousData });
  const onErr = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const done = () => { toast.success(t("apikeys.revoked")); void qc.invalidateQueries({ queryKey: ["admin", "api-keys"] }); void qc.invalidateQueries({ queryKey: ["api-keys"] }); };
  const revoke = useMutation({ mutationFn: (id: string) => api(`/admin/api-keys/${id}`, { method: "DELETE" }), onSuccess: done, onError: onErr });
  const revokeAll = useMutation({ mutationFn: (uid: string) => api(`/admin/users/${uid}/api-keys/revoke-all`, { method: "POST" }), onSuccess: done, onError: onErr });
  const pages = data ? Math.max(1, Math.ceil(data.total / data.per_page)) : 1;
  const status = (k: OwnedKey) => k.revoked_at ? <Badge variant="secondary">{t("apikeys.status_revoked")}</Badge>
    : k.expires_at && new Date(k.expires_at) < new Date() ? <Badge variant="outline">{t("apikeys.status_expired")}</Badge>
    : <Badge>{t("apikeys.status_active")}</Badge>;
  return (
    <div className="flex flex-col gap-3">
    <Card>
      <CardHeader>
        <CardTitle>{t("apikeys.all_title")}</CardTitle>
        <CardDescription>{t("apikeys.all_hint")}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3 sm:flex-row sm:items-center">
        <Input type="search" aria-label={t("apikeys.search")} placeholder={t("apikeys.search")} value={q} onChange={(e) => { setQ(e.target.value); setPage(1); }} className="sm:max-w-xs" />
        <div className="flex items-center gap-2">
          <Switch id="keys-active" checked={active} onCheckedChange={(v) => { setActive(v); setPage(1); }} />
          <Label htmlFor="keys-active">{t("apikeys.active_only")}</Label>
        </div>
      </CardContent>
    </Card>
      <ListCard<OwnedKey>
        rows={data?.items} rowKey={(k) => k.id} loading={isLoading} emptyIcon={KeyIcon} emptyTitle={t("apikeys.empty")}
        columns={[
          { header: t("apikeys.name"), primary: true, cell: (k) => <span className="flex flex-col"><span className="font-medium">{k.name}</span><code className="text-xs text-muted-foreground">{k.prefix}…</code></span> },
          { header: t("apikeys.owner"), cell: (k) => <span className="break-all">{k.user_email}</span> },
          { header: t("apikeys.scopes"), cell: (k) => <span className="flex flex-wrap gap-1">{k.scopes.map((s) => <Badge key={s} variant="outline" className="font-mono text-[11px]">{s}</Badge>)}</span> },
          { header: t("apikeys.allowed_ips"), cell: (k) => <IpBadges ips={k.allowed_ips ?? []} /> },
          { header: t("apikeys.status"), cell: status },
          { header: t("apikeys.last_used"), cell: (k) => (k.last_used_at ? formatDateTime(k.last_used_at, lang) : t("apikeys.never")) },
          { header: "", className: "text-right", cell: (k) => (
            <span className="flex flex-wrap justify-end gap-1">
              {!k.revoked_at && <KeyIpButton k={{ id: k.id, allowed_ips: k.allowed_ips ?? [] }} base="/admin/api-keys" onSaved={() => void qc.invalidateQueries({ queryKey: ["admin", "api-keys"] })} />}
              {!k.revoked_at && (
                <ConfirmAction description={t("apikeys.confirm_revoke")} confirmLabel={t("apikeys.revoke")} onConfirm={() => revoke.mutate(k.id)}>
                  <Button size="sm" variant="ghost" disabled={revoke.isPending}><ProhibitIcon aria-hidden />{t("apikeys.revoke")}</Button>
                </ConfirmAction>
              )}
              <ConfirmAction description={t("apikeys.confirm_revoke_all", { email: k.user_email })} confirmLabel={t("apikeys.revoke_all")} onConfirm={() => revokeAll.mutate(k.user_id)}>
                <Button size="sm" variant="ghost" disabled={revokeAll.isPending}>{t("apikeys.revoke_all")}</Button>
              </ConfirmAction>
            </span>
          ) },
        ]}
      />
      {data && data.total > data.per_page && (
        <div className="flex items-center justify-end gap-2">
          <Button size="icon" variant="outline" aria-label={t("apikeys.prev")} disabled={page <= 1} onClick={() => setPage(page - 1)}><CaretLeftIcon aria-hidden /></Button>
          <span className="text-sm text-muted-foreground">{page} / {pages}</span>
          <Button size="icon" variant="outline" aria-label={t("apikeys.next")} disabled={page >= pages} onClick={() => setPage(page + 1)}><CaretRightIcon aria-hidden /></Button>
        </div>
      )}
    </div>
  );
}

export function AdminApiKeysPage() {
  return (
    <div className="flex flex-col gap-6">
      <ApiKeysPage />
      <AllApiKeysCard />
    </div>
  );
}
