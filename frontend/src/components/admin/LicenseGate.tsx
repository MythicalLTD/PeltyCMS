import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ScrollIcon, ScalesIcon, ArrowSquareOutIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { useState } from "react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
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
  free_limit: number;
  active_clients: number;
  over_free_limit: boolean;
  enforcement: string;
  accepted: boolean;
  show_accept_gate: boolean;
  show_overage_banner: boolean;
  commercial_url: string;
  eula_url: string;
  license_url: string;
  summary: { free_tier: string; overage: string; soft: string };
}

export const LICENSE_KEY = ["admin", "license"] as const;
export const useLicense = () => useQuery({ queryKey: LICENSE_KEY, queryFn: () => api<LicenseStatus>("/admin/license"), retry: false, staleTime: 60_000 });

/** Blocking dialog until LICENSE + EULA are accepted for this deployment. */
export function LicenseAcceptGate() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useLicense();
  const [okLicense, setOkLicense] = useState(false);
  const [okEula, setOkEula] = useState(false);
  const [okCommercial, setOkCommercial] = useState(false);
  const accept = useMutation({
    mutationFn: () => api<LicenseStatus>("/admin/license/accept", { method: "POST", body: { accept: true } }),
    onSuccess: (d) => { qc.setQueryData(LICENSE_KEY, d); toast.success(t("license.accepted")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const open = !!q.data?.show_accept_gate;
  const ready = okLicense && okEula && okCommercial;
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
            <p>{q.data.summary.free_tier}</p>
            <p>{q.data.summary.overage}</p>
            <p className="text-muted-foreground">{q.data.summary.soft}</p>
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
              <div className="flex items-start gap-2">
                <Checkbox id="lic-commercial" checked={okCommercial} onCheckedChange={(v) => setOkCommercial(v === true)} />
                <Label htmlFor="lic-commercial" className="cursor-pointer leading-snug font-normal">{t("license.check_commercial", { limit: q.data.free_limit })}</Label>
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

/** Soft banner when Active Clients exceed the Free Tier. Never blocks the UI. */
export function LicenseOverageBanner() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useLicense();
  const dismiss = useMutation({
    mutationFn: () => api<LicenseStatus>("/admin/license/overage-notice", { method: "PUT", body: { dismissed: true } }),
    onSuccess: (d) => { qc.setQueryData(LICENSE_KEY, d); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (!q.data?.show_overage_banner) return null;
  return (
    <Alert data-testid="license-overage-banner">
      <ScalesIcon aria-hidden />
      <AlertTitle>{t("license.overage_title")}</AlertTitle>
      <AlertDescription className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <span>{t("license.overage_text", { count: q.data.active_clients, limit: q.data.free_limit })}</span>
        <div className="flex flex-wrap gap-2">
          <Button asChild size="sm" variant="outline">
            <a href={q.data.commercial_url} target="_blank" rel="noreferrer noopener">{t("license.get_commercial")}<ArrowSquareOutIcon aria-hidden /></a>
          </Button>
          <Button type="button" size="sm" variant="ghost" disabled={dismiss.isPending} onClick={() => dismiss.mutate()}>
            {t("license.dismiss_notice")}
          </Button>
        </div>
      </AlertDescription>
    </Alert>
  );
}
