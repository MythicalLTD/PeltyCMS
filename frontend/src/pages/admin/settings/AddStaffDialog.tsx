import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { UserPlusIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Field, FormError } from "@/components/Field";
import { SelectField } from "@/components/SelectField";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { Role } from "@/pages/admin/settings/Staff";

/** Add a staff member: invite a new account by email, set its password, or promote an existing account. */
export function AddStaffDialog({ roles }: { roles: Role[] }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [email, setEmail] = useState("");
  const [name, setName] = useState("");
  const [login, setLogin] = useState<"invite" | "password">("invite");
  const [password, setPassword] = useState("");
  const [sel, setSel] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const staffRoles = roles.filter((r) => r.name !== "customer");
  const m = useMutation({
    mutationFn: () => api<{ id: string; invited: boolean }>("/admin/staff", { method: "POST", body: { email: email.trim(), display_name: name.trim() || undefined, roles: sel, login, password: login === "password" ? password : undefined } }),
    onSuccess: (r) => {
      toast.success(t(r.invited ? "staff.invited" : "staff.added"));
      void qc.invalidateQueries({ queryKey: ["admin", "staff"] });
      void qc.invalidateQueries({ queryKey: ["admin", "roles"] });
      setOpen(false);
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { setEmail(""); setName(""); setLogin("invite"); setPassword(""); setSel([]); setError(null); } }}>
      <DialogTrigger asChild><Button><UserPlusIcon aria-hidden />{t("staff.add")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); m.mutate(); }}>
          <DialogHeader><DialogTitle>{t("staff.add")}</DialogTitle><DialogDescription>{t("staff.add_help")}</DialogDescription></DialogHeader>
          <Field id="as-email" type="email" required label={t("field.email")} value={email} onChange={(e) => setEmail(e.target.value)} />
          <Field id="as-name" label={t("staff.display_name")} value={name} onChange={(e) => setName(e.target.value)} />
          <div className="grid gap-2">
            <Label>{t("staff.roles")}</Label>
            <div className="grid gap-2 rounded-md border p-3">
              {staffRoles.map((r) => (
                <Label key={r.id} className="flex items-center gap-2 font-normal">
                  <Checkbox checked={sel.includes(r.name)} onCheckedChange={(v) => setSel((s) => (v === true ? [...s, r.name] : s.filter((x) => x !== r.name)))} />{r.name}
                </Label>
              ))}
            </div>
          </div>
          <SelectField id="as-login" label={t("clients.login")} value={login} onChange={(v) => setLogin(v as "invite" | "password")} options={(["invite", "password"] as const).map((v) => ({ value: v, label: t(`clients.login_${v}`) }))} />
          {login === "password" && <Field id="as-pw" type="password" autoComplete="new-password" required label={t("field.password")} value={password} onChange={(e) => setPassword(e.target.value)} />}
          <p className="text-sm text-muted-foreground">{t("staff.add_existing_help")}</p>
          <FormError message={error} />
          <DialogFooter><Button type="submit" disabled={m.isPending || !email.trim() || sel.length === 0 || (login === "password" && !password)}>{t("staff.add")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
