import { useState } from "react";
import { AddonMixinsCard } from "@/pages/admin/extensions/AddonMixins";
import { PageSkeleton, BlockSkeleton } from "@/components/PageSkeleton";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { CreditCardIcon, ComputerTowerIcon, PaletteIcon, GlobeIcon, PuzzlePieceIcon, ShieldCheckIcon, RobotIcon, SignInIcon, PlugsConnectedIcon, ShieldWarningIcon, MagnifyingGlassIcon, CubeIcon, WarningIcon, type Icon } from "@phosphor-icons/react";
import { PageHeader } from "@/components/PageHeader";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Checkbox } from "@/components/ui/checkbox";
import { Textarea } from "@/components/ui/textarea";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { ConfirmAction } from "@/components/ConfirmAction";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { UploadAddonButton, MarketplaceButton } from "./AddonPackages";
import { capabilityLines, dangerLine, describe, filterAddons, initialValues, type Addon, type AddonFilter, type SettingSpec } from "@/lib/addons/addons";
import type { TranslationKey } from "@/lib/i18n-core";

const KIND_ICON: Record<Addon["kind"], Icon> = { provisioning: ComputerTowerIcon, gateway: CreditCardIcon, theme: PaletteIcon, plugin: PuzzlePieceIcon, registrar: GlobeIcon, captcha: RobotIcon, sso: SignInIcon, integration: PlugsConnectedIcon };
const FILTERS: AddonFilter[] = ["all", "installed", "available", "provisioning", "gateway", "registrar", "plugin", "theme", "captcha", "sso", "integration"];

/** Installable WASM modules / engines. `embedded` hides the page chrome when shown under Extensions. */
export function AdminAddonsPage({ embedded = false }: { embedded?: boolean } = {}) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [filter, setFilter] = useState<AddonFilter>("all");
  const [q, setQ] = useState("");
  const [editing, setEditing] = useState<Addon | null>(null);
  const [reviewing, setReviewing] = useState<Addon | null>(null);
  const { data, isLoading } = useQuery({ queryKey: ["admin", "addons"], queryFn: () => api<{ items: Addon[] }>("/admin/addons") });
  const act = useMutation({
    mutationFn: ({ a, op, approve }: { a: Addon; op: "install" | "enable" | "disable" | "uninstall"; approve?: string[] }) =>
      op === "uninstall" ? api<void>(`/admin/addons/${a.id}`, { method: "DELETE" }) : api<unknown>(`/admin/addons/${a.id}/${op}`, { method: "POST", body: approve?.length ? { approve } : undefined }),
    onSuccess: (_d, v) => { toast.success(t(`addons.done.${v.op}`)); void qc.invalidateQueries({ queryKey: ["admin", "addons"] }); },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : t("error.internal")),
  });
  const items = filterAddons(data?.items ?? [], filter, q);
  return (
    <div className="flex min-w-0 flex-col gap-4">
      {!embedded && <PageHeader title={t("addons.title")} description={t("addons.subtitle")} actions={<div className="flex flex-wrap gap-2"><UploadAddonButton /><MarketplaceButton /></div>} />}
      {embedded && (
        <div className="flex flex-wrap justify-end gap-2"><UploadAddonButton /><MarketplaceButton /></div>
      )}
      <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <Tabs value={filter} onValueChange={(v) => setFilter(v as AddonFilter)} className="min-w-0 overflow-x-auto">
          <TabsList>{FILTERS.map((f) => <TabsTrigger key={f} value={f}>{t(`addons.filter.${f}`)}</TabsTrigger>)}</TabsList>
        </Tabs>
        <div className="relative md:w-64">
          <MagnifyingGlassIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input className="pl-8" placeholder={t("addons.search")} value={q} onChange={(e) => setQ(e.target.value)} aria-label={t("addons.search")} />
        </div>
      </div>
      <AddonMixinsCard />
      {isLoading && <PageSkeleton />}
      {!isLoading && items.length === 0 && <p className="text-sm text-muted-foreground">{t("addons.empty")}</p>}
      <div className="grid min-w-0 gap-3 md:grid-cols-2 xl:grid-cols-3">
        {items.map((a) => {
          const KindIcon = KIND_ICON[a.kind];
          const busy = act.isPending && act.variables?.a.id === a.id;
          return (
            <Card key={a.id} className="min-w-0 shadow-xs">
              <CardContent className="flex h-full flex-col gap-3">
                <div className="flex items-start gap-3">
                  <div className="flex size-10 shrink-0 items-center justify-center rounded-md border bg-muted"><KindIcon className="size-5" /></div>
                  <div className="min-w-0 flex-1">
                    <p className="line-clamp-2 break-words font-medium" title={a.name}>{a.name}</p>
                    <p className="truncate font-mono text-xs text-muted-foreground">{a.id} · v{a.installed_version ?? a.version}</p>
                  </div>
                  <Badge variant={a.status === "enabled" ? "default" : a.status === "disabled" ? "outline" : "muted"}>{t(`addons.status.${a.status}`)}</Badge>
                </div>
                <p className="text-sm text-muted-foreground">{describe(a, tCode)}</p>
                <div className="flex flex-wrap gap-1.5">
                  <Badge variant="outline">{t(`addons.kind.${a.kind}`)}</Badge>
                  <Badge variant="muted">{t(`addons.source.${a.source}`)}</Badge>
                  {a.wasm && <Badge variant="outline" className="gap-1"><CubeIcon className="size-3" />WASM</Badge>}
                  {a.permissions.map((p) => <Badge key={p} variant="outline" className="font-mono text-[11px]">{p}</Badge>)}
                </div>
                <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
                  {a.integrity_ok ? <ShieldCheckIcon className="size-4" /> : <ShieldWarningIcon className="size-4 text-destructive" />}
                  {a.integrity_ok ? t("addons.integrity_ok") : t("addons.integrity_bad")}
                  {a.source === "disk" && <span className="truncate font-mono">{a.hash.slice(0, 12)}</span>}
                </p>
                <div className="mt-auto flex flex-wrap gap-2 pt-1">
                  {a.status === "available" ? (
                    <Button size="sm" disabled={busy} onClick={() => (a.wasm ? setReviewing(a) : act.mutate({ a, op: "install" }))}>{t("addons.install")}</Button>
                  ) : (
                    <>
                      <Button size="sm" variant={a.status === "enabled" ? "outline" : "default"} disabled={busy} onClick={() => act.mutate({ a, op: a.status === "enabled" ? "disable" : "enable" })}>
                        {a.status === "enabled" ? t("addons.disable") : t("addons.enable")}
                      </Button>
                      {a.settings_schema.length > 0 && <Button size="sm" variant="ghost" onClick={() => setEditing(a)}>{t("addons.settings")}</Button>}
                      {a.wasm && <Button size="sm" variant="ghost" onClick={() => setReviewing(a)}>{t("addons.cap.button")}</Button>}
                      <ConfirmAction title={t("addons.uninstall_confirm")} description={t("addons.uninstall_hint")} onConfirm={() => act.mutate({ a, op: "uninstall" })}>
                        <Button size="sm" variant="ghost" className="text-destructive" disabled={busy}>{t("addons.uninstall")}</Button>
                      </ConfirmAction>
                    </>
                  )}
                </div>
              </CardContent>
            </Card>
          );
        })}
      </div>
      {editing && <SettingsDialog addon={editing} onClose={() => setEditing(null)} />}
      {reviewing && (
        <PermissionsDialog
          addon={reviewing}
          busy={act.isPending}
          onClose={() => setReviewing(null)}
          onInstall={reviewing.status === "available" ? (approve: string[]) => act.mutate({ a: reviewing, op: "install", approve }, { onSettled: () => setReviewing(null) }) : undefined}
        />
      )}
    </div>
  );
}

/** What a WASM addon may do (approved at install): network hosts, secrets, storage, events, permissions. */
export function PermissionsDialog({ addon, busy, onClose, onInstall }: { addon: Addon; busy: boolean; onClose: () => void; onInstall?: (approve: string[]) => void }) {
  const { t } = useI18n();
  const lines = capabilityLines(addon.capabilities);
  const dangerous = addon.capabilities?.dangerous ?? [];
  const [approved, setApproved] = useState<string[]>([]);
  const allApproved = dangerous.every((d) => approved.includes(d));
  const limits = addon.capabilities?.limits;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("addons.cap.title", { name: addon.name })}</DialogTitle>
          <DialogDescription>{t("addons.cap.hint")}</DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3 text-sm">
          <p className="font-mono text-xs text-muted-foreground">{addon.id} · {addon.provides} · ABI {addon.abi}</p>
          {lines.length === 0 ? (
            <p className="text-muted-foreground">{t("addons.cap.none")}</p>
          ) : (
            <ul className="flex list-disc flex-col gap-1 pl-5">
              {lines.map(([k, vars], i) => <li key={i} className="break-words">{t(k as TranslationKey, vars)}</li>)}
            </ul>
          )}
          {addon.permissions.length > 0 && (
            <div className="flex flex-col gap-1.5">
              <p className="font-medium">{t("addons.cap.permissions")}</p>
              <div className="flex flex-wrap gap-1.5">{addon.permissions.map((p) => <Badge key={p} variant="outline" className="font-mono text-[11px]">{p}</Badge>)}</div>
            </div>
          )}
          {limits && (limits.memory_mib || limits.timeout_secs) && (
            <p className="text-xs text-muted-foreground">{t("addons.cap.limits", { memory: String(limits.memory_mib ?? 64), timeout: String(limits.timeout_secs ?? 5) })}</p>
          )}
          {dangerous.length > 0 && (
            <Alert variant="destructive">
              <WarningIcon aria-hidden />
              <AlertDescription className="flex flex-col gap-2">
                <span className="font-medium">{t("addons.cap.danger_title")}</span>
                {dangerous.map((d) => {
                  const [k, vars] = dangerLine(d, addon.capabilities);
                  return (
                    <span key={d} className="flex items-start gap-2">
                      {onInstall && <Checkbox id={`approve-${d}`} className="mt-0.5" checked={approved.includes(d)} onCheckedChange={(v) => setApproved((a) => (v === true ? [...a, d] : a.filter((x) => x !== d)))} />}
                      <Label htmlFor={`approve-${d}`} className="font-normal leading-snug"><span className="font-mono text-[11px]">{d}</span>, {t(k as TranslationKey, vars)}</Label>
                    </span>
                  );
                })}
                {addon.capabilities?.exec_blocked && <span>{t("addons.cap.exec_blocked")}</span>}
              </AlertDescription>
            </Alert>
          )}
          <p className="text-xs text-muted-foreground">{t("addons.cap.sandbox")}</p>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>{onInstall ? t("common.cancel") : t("common.close")}</Button>
          {onInstall && <Button disabled={busy || !allApproved} onClick={() => onInstall(approved)}>{t("addons.cap.approve")}</Button>}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function SettingsDialog({ addon, onClose }: { addon: Addon; onClose: () => void }) {
  const { t } = useI18n();
  const { data } = useQuery({ queryKey: ["admin", "addons", addon.id, "settings"], queryFn: () => api<{ schema: SettingSpec[]; values: Record<string, unknown> }>(`/admin/addons/${addon.id}/settings`) });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader><DialogTitle>{t("addons.settings")}: {addon.name}</DialogTitle></DialogHeader>
        {data ? <SettingsForm schema={data.schema} values={data.values} id={addon.id} onDone={onClose} /> : <BlockSkeleton />}
      </DialogContent>
    </Dialog>
  );
}

function SettingsForm({ schema, values, id, onDone }: { schema: SettingSpec[]; values: Record<string, unknown>; id: string; onDone: () => void }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [v, setV] = useState(() => initialValues(schema, values));
  const save = useMutation({
    mutationFn: () => api<unknown>(`/admin/addons/${id}/settings`, { method: "PUT", body: Object.fromEntries(Object.entries(v).filter(([, x]) => x !== "")) }),
    onSuccess: () => { toast.success(t("common.saved")); void qc.invalidateQueries({ queryKey: ["admin", "addons"] }); onDone(); },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : t("error.internal")),
  });
  const set = (k: string, x: unknown) => setV((o) => ({ ...o, [k]: x }));
  return (
    <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
      {schema.map((s) => (
        <div key={s.key} className="flex flex-col gap-1.5">
          <Label htmlFor={`f-${s.key}`}>{s.label ? tCode(s.label) : s.key}{s.required && " *"}</Label>
          {s.kind === "bool" ? (
            <Switch id={`f-${s.key}`} checked={Boolean(v[s.key])} onCheckedChange={(c) => set(s.key, c)} />
          ) : s.kind === "select" ? (
            <Select value={String(v[s.key] ?? "")} onValueChange={(x) => set(s.key, x)}>
              <SelectTrigger id={`f-${s.key}`}><SelectValue /></SelectTrigger>
              <SelectContent>{s.choices.map((c) => <SelectItem key={c} value={c}>{c}</SelectItem>)}</SelectContent>
            </Select>
          ) : s.kind === "textarea" ? (
            <Textarea id={`f-${s.key}`} value={String(v[s.key] ?? "")} onChange={(e) => set(s.key, e.target.value)} />
          ) : (
            <Input id={`f-${s.key}`} type={s.kind === "number" ? "number" : s.kind === "secret" ? "password" : "text"} autoComplete={s.kind === "secret" ? "new-password" : undefined} required={s.required} value={String(v[s.key] ?? "")} onChange={(e) => set(s.key, s.kind === "number" && e.target.value !== "" ? Number(e.target.value) : e.target.value)} />
          )}
        </div>
      ))}
      <DialogFooter><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></DialogFooter>
    </form>
  );
}
