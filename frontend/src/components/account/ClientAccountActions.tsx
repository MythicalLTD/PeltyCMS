import { useState } from "react";
import { useNavigate } from "react-router";
import { useQueryClient } from "@tanstack/react-query";
import { ArrowsMergeIcon, LockIcon, UserSwitchIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { ConfirmAction } from "@/components/ConfirmAction";
import { CustomerPicker } from "@/components/admin/CustomerPicker";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { api, ApiError, setCsrfToken } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

/** Admin actions on a client: sign in as client, close account, merge into another client. */
export function ClientAccountActions({ customerId, closed }: { customerId: string; closed?: boolean }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const nav = useNavigate();
  const [into, setInto] = useState("");
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const loginAs = async () => {
    try {
      const r = await api<{ csrf_token: string; redirect: string }>(`/admin/customers/${customerId}/login-as`, { method: "POST" });
      setCsrfToken(r.csrf_token);
      window.location.assign(r.redirect);
    } catch (e) { fail(e); }
  };
  const close = async () => {
    try { await api(`/admin/customers/${customerId}/close`, { method: "POST" }); toast.success(t("client_actions.closed")); void qc.invalidateQueries({ queryKey: ["admin"] }); } catch (e) { fail(e); }
  };
  const merge = async () => {
    setBusy(true);
    try {
      await api(`/admin/customers/${customerId}/merge`, { method: "POST", body: { into } });
      toast.success(t("client_actions.merged"));
      setOpen(false);
      nav(`/admin/customers/${into}`);
    } catch (e) { fail(e); } finally { setBusy(false); }
  };
  return (
    <div className="flex flex-wrap gap-2">
      <Button size="sm" variant="outline" onClick={() => void loginAs()} disabled={closed}><UserSwitchIcon />{t("client_actions.login_as")}</Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogTrigger asChild><Button size="sm" variant="outline"><ArrowsMergeIcon />{t("client_actions.merge")}</Button></DialogTrigger>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("client_actions.merge")}</DialogTitle><DialogDescription>{t("client_actions.merge_hint")}</DialogDescription></DialogHeader>
          <CustomerPicker value={into} onChange={setInto} />
          <DialogFooter><Button variant="destructive" disabled={!into || into === customerId || busy} onClick={() => void merge()}>{t("client_actions.merge_confirm")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
      {!closed && (
        <ConfirmAction description={t("client_actions.close_hint")} confirmLabel={t("client_actions.close")} onConfirm={() => void close()}>
          <Button size="sm" variant="outline" className="text-destructive hover:text-destructive"><LockIcon />{t("client_actions.close")}</Button>
        </ConfirmAction>
      )}
    </div>
  );
}
