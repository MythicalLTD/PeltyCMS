import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { FingerprintIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { ConfirmAction } from "@/components/ConfirmAction";
import { StepUpCodeField, useNeedsCode } from "@/components/account/StepUpCodeField";
import { usePasskeysAvailable } from "@/lib/admin/general";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { enrolPasskey, passkeysSupported, type PasskeyRow } from "@/lib/passkeys";

const KEY = ["auth", "passkeys"];
const SF_KEY = ["auth", "mfa", "passkey"];

export interface SecondFactorStatus { enabled: boolean; passkeys: number; totp: boolean }

/** Own passkeys: add (password re-entry), list, remove. */
export function PasskeysCard() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: KEY, queryFn: () => api<{ items: PasskeyRow[] }>("/auth/passkeys") });
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const needsCode = useNeedsCode();
  const err = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("passkeys.cancelled"));
  const add = useMutation({
    mutationFn: () => enrolPasskey(password, code, name),
    onSuccess: () => { toast.success(t("passkeys.added")); setOpen(false); setPassword(""); setCode(""); setName(""); void qc.invalidateQueries({ queryKey: KEY }); },
    onError: err,
  });
  const sf = useQuery({ queryKey: SF_KEY, queryFn: () => api<SecondFactorStatus>("/auth/mfa/passkey") });
  const [sfOpen, setSfOpen] = useState(false);
  const [sfPw, setSfPw] = useState("");
  const [sfCode, setSfCode] = useState("");
  const setSf = useMutation({
    mutationFn: (enabled: boolean) => api<{ enabled: boolean }>("/auth/mfa/passkey", { method: "PUT", body: { enabled, password: sfPw, code: sfCode } }),
    onSuccess: (r) => {
      toast.success(t(r.enabled ? "passkeys.second_factor_on" : "passkeys.second_factor_off"));
      setSfOpen(false); setSfPw(""); setSfCode("");
      void qc.invalidateQueries({ queryKey: SF_KEY });
      void qc.invalidateQueries({ queryKey: ["me"] });
    },
    onError: (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("common.error")),
  });
  const sfOn = sf.data?.enabled === true;
  const del = useMutation({
    mutationFn: (id: string) => api(`/auth/passkeys/${id}`, { method: "DELETE" }),
    onSuccess: () => { void qc.invalidateQueries({ queryKey: KEY }); void qc.invalidateQueries({ queryKey: SF_KEY }); },
    onError: err,
  });
  const items = q.data?.items ?? [];
  const browserOk = passkeysSupported();
  const supported = usePasskeysAvailable();
  return (
    <Card>
      <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-2">
        <div className="grid gap-1.5">
          <CardTitle className="flex items-center gap-2"><FingerprintIcon aria-hidden />{t("passkeys.title")}</CardTitle>
          <CardDescription>{t("passkeys.help")}</CardDescription>
        </div>
        <Dialog open={open} onOpenChange={setOpen}>
          <DialogTrigger asChild><Button size="sm" disabled={!supported}><PlusIcon aria-hidden />{t("passkeys.add")}</Button></DialogTrigger>
          <DialogContent>
            <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
              <DialogHeader><DialogTitle>{t("passkeys.add")}</DialogTitle><DialogDescription>{t(needsCode ? "passkeys.add_help" : "passkeys.add_help_pw")}</DialogDescription></DialogHeader>
              <div className="grid gap-2"><Label htmlFor="pk-name">{t("passkeys.name")}</Label><Input id="pk-name" maxLength={100} value={name} placeholder={t("passkeys.name_placeholder")} onChange={(e) => setName(e.target.value)} required /></div>
              <div className="grid gap-2"><Label htmlFor="pk-pw">{t("reauth.password")}</Label><Input id="pk-pw" type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} required /></div>
              <StepUpCodeField id="pk-code" value={code} onChange={setCode} />
              <DialogFooter><Button type="submit" disabled={add.isPending || !password || !name.trim()}>{t("passkeys.create")}</Button></DialogFooter>
            </form>
          </DialogContent>
        </Dialog>
      </CardHeader>
      <CardContent className="grid gap-2">
        {!supported && <p className="text-sm text-muted-foreground">{browserOk ? tCode("error.passkeys_unavailable") : t("passkeys.unsupported")}</p>}
        {items.length === 0 ? <p className="text-sm text-muted-foreground">{t("passkeys.none")}</p> : (
          <ul className="grid gap-2">
            {items.map((p) => (
              <li key={p.id} className="flex flex-wrap items-center justify-between gap-2 rounded-md border p-3 text-sm">
                <span className="grid min-w-0">
                  <span className="truncate font-medium">{p.name}</span>
                  <span className="text-xs text-muted-foreground">{t("passkeys.added_at", { at: formatDateTime(p.created_at, lang) })} · {p.last_used_at ? t("passkeys.used_at", { at: formatDateTime(p.last_used_at, lang) }) : t("passkeys.never_used")}</span>
                </span>
                <ConfirmAction description={t("passkeys.remove_confirm")} onConfirm={() => del.mutate(p.id)}>
                  <Button size="sm" variant="outline" aria-label={t("common.delete")}><TrashIcon aria-hidden />{t("common.delete")}</Button>
                </ConfirmAction>
              </li>
            ))}
          </ul>
        )}
        {items.length > 0 && sf.data && (
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-md border p-3">
            <div className="grid gap-0.5">
              <Label htmlFor="pk-2fa">{t("passkeys.second_factor")}</Label>
              <span className="text-xs text-muted-foreground">{sf.data.totp ? t("passkeys.second_factor_with_totp") : t("passkeys.second_factor_help")}</span>
            </div>
            <Switch id="pk-2fa" checked={sfOn} onCheckedChange={() => setSfOpen(true)} />
            <Dialog open={sfOpen} onOpenChange={setSfOpen}>
              <DialogContent>
                <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); setSf.mutate(!sfOn); }}>
                  <DialogHeader><DialogTitle>{sfOn ? t("passkeys.second_factor_disable") : t("passkeys.second_factor_enable")}</DialogTitle><DialogDescription>{t(needsCode ? "reauth.hint_code" : "reauth.hint_pw")}</DialogDescription></DialogHeader>
                  <div className="grid gap-2"><Label htmlFor="sf-pw">{t("reauth.password")}</Label><Input id="sf-pw" type="password" autoComplete="current-password" value={sfPw} onChange={(e) => setSfPw(e.target.value)} required /></div>
                  <StepUpCodeField id="sf-code" value={sfCode} onChange={setSfCode} />
                  <DialogFooter><Button type="submit" disabled={setSf.isPending || !sfPw}>{t("common.save")}</Button></DialogFooter>
                </form>
              </DialogContent>
            </Dialog>
          </div>
        )}
        <p className="text-xs text-muted-foreground">{t("passkeys.recovery")}</p>
      </CardContent>
    </Card>
  );
}
