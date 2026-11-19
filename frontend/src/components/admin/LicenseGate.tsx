import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ScrollIcon, ArrowSquareOutIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

export interface LicenseStatus {
  version: string;
  /** SPDX id (`AGPL-3.0-or-later`). */
  license: string;
  accepted: boolean;
  accepted_version: string;
  show_accept_gate: boolean;
  commercial_url: string;
  eula_url: string;
  license_url: string;
  source_url: string;
}

export const LICENSE_KEY = ["admin", "license"] as const;
export const useLicense = () => useQuery({ queryKey: LICENSE_KEY, queryFn: () => api<LicenseStatus>("/admin/license"), retry: false, staleTime: 60_000 });

/** Blocking dialog until the license notice (LICENSE + EULA.md) is acknowledged for this deployment. */
export function LicenseAcceptGate() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useLicense();
  const [okLicense, setOkLicense] = useState(false);
  const [okEula, setOkEula] = useState(false);
  const accept = useMutation({
    mutationFn: () => api<LicenseStatus>("/admin/license/accept", { method: "POST", body: { accept: true } }),
    onSuccess: (d) => { qc.setQueryData(LICENSE_KEY, d); toast.success(t("license.accepted")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const open = !!q.data?.show_accept_gate;
  const ready = okLicense && okEula;
  return (
    <Dialog open={open} onOpenChange={() => { /* cannot dismiss without accept */ }}>
      <DialogContent
        className="max-h-[90dvh] overflow-y-auto sm:max-w-lg"
        showCloseButton={false}
        onPointerDownOutside={(e) => e.preventDefault()}
        onEscapeKeyDown={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2"><ScrollIcon aria-hidden className="size-5" />{t("license.gate_title")}</DialogTitle>
          <DialogDescription>{t("license.gate_desc")}</DialogDescription>
        </DialogHeader>
        {q.data && (
          <div className="flex flex-col gap-4 text-sm">
            <p>{t("license.summary_free")}</p>
            <p className="text-muted-foreground">{t("license.summary_premium")}</p>
            <ul className="flex flex-col gap-2">
              <li>
                <a className="inline-flex items-center gap-1 text-primary underline-offset-4 hover:underline" href={q.data.license_url} target="_blank" rel="noreferrer noopener">
                  {t("license.doc_license")}<ArrowSquareOutIcon aria-hidden className="size-3.5" />
                </a>
                <span className="text-muted-foreground"> · LICENSE in the install</span>
              </li>
              <li>
                <a className="inline-flex items-center gap-1 text-primary underline-offset-4 hover:underline" href={q.data.eula_url} target="_blank" rel="noreferrer noopener">
                  {t("license.doc_eula")}<ArrowSquareOutIcon aria-hidden className="size-3.5" />
                </a>
                <span className="text-muted-foreground"> · EULA.md</span>
              </li>
              <li>
                <a className="inline-flex items-center gap-1 text-primary underline-offset-4 hover:underline" href={q.data.commercial_url} target="_blank" rel="noreferrer noopener">
                  {t("license.doc_commercial")}<ArrowSquareOutIcon aria-hidden className="size-3.5" />
                </a>
                <span className="text-muted-foreground"> · COMMERCIAL.md</span>
              </li>
            </ul>
            <div className="flex flex-col gap-3 rounded-md border p-3">
              <div className="flex items-start gap-2">
                <Checkbox id="lic-license" checked={okLicense} onCheckedChange={(v) => setOkLicense(v === true)} />
                <Label htmlFor="lic-license" className="cursor-pointer leading-snug font-normal">{t("license.check_license")}</Label>
              </div>
              <div className="flex items-start gap-2">
                <Checkbox id="lic-eula" checked={okEula} onCheckedChange={(v) => setOkEula(v === true)} />
                <Label htmlFor="lic-eula" className="cursor-pointer leading-snug font-normal">{t("license.check_eula")}</Label>
              </div>
            </div>
          </div>
        )}
        <DialogFooter>
          <Button type="button" disabled={!ready || accept.isPending || !q.data} onClick={() => accept.mutate()}>
            {t("license.accept")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
