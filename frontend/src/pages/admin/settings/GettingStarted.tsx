import { Link } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircleIcon, CircleIcon, RocketLaunchIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { LICENSE_KEY, useLicense, type LicenseStatus } from "@/components/admin/LicenseGate";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";

export interface WizardStep { id: string; done: boolean; optional: boolean; href: string }
export interface Wizard { steps: WizardStep[]; complete: boolean; dismissed: boolean; show: boolean }

/** Required steps done, as "done/total". */
export function requiredProgress(w: Wizard): [number, number] {
  const req = w.steps.filter((s) => !s.optional);
  return [req.filter((s) => s.done).length, req.length];
}

const KEY = ["admin", "setup-wizard"];
const useWizard = () => useQuery({ queryKey: KEY, queryFn: () => api<Wizard>("/admin/setup-wizard"), retry: false });

/** Dashboard banner shown on first run until setup is done or dismissed. */
export function SetupWizardBanner() {
  const { t } = useI18n();
  const q = useWizard();
  if (!q.data?.show) return null;
  const [done, total] = requiredProgress(q.data);
  return (
    <Alert>
      <RocketLaunchIcon />
      <AlertTitle>{t("setup.banner_title")}</AlertTitle>
      <AlertDescription className="flex flex-wrap items-center justify-between gap-2">
        <span>{t("setup.banner_text", { done, total })}</span>
        <Button asChild size="sm"><Link to="/admin/getting-started">{t("setup.open")}</Link></Button>
      </AlertDescription>
    </Alert>
  );
}

export function AdminGettingStartedPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useWizard();
  const lic = useLicense();
  const dismiss = useMutation({
    mutationFn: (dismissed: boolean) => api<Wizard>("/admin/setup-wizard/dismissed", { method: "PUT", body: { dismissed } }),
    onSuccess: (d) => { qc.setQueryData(KEY, d); toast.success(t("common.saved")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const acceptLic = useMutation({
    mutationFn: () => api<LicenseStatus>("/admin/license/accept", { method: "POST", body: { accept: true } }),
    onSuccess: async (d) => {
      qc.setQueryData(LICENSE_KEY, d);
      await qc.invalidateQueries({ queryKey: KEY });
      toast.success(t("license.accepted"));
    },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (!q.data) return <Skeleton className="h-64" />;
  const w = q.data;
  const [done, total] = requiredProgress(w);
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("nav.getting_started")} description={w.complete ? t("setup.complete") : t("setup.banner_text", { done, total })}
        actions={<Button variant="outline" disabled={dismiss.isPending} onClick={() => dismiss.mutate(!w.dismissed)}>{w.dismissed ? t("setup.restore") : t("setup.dismiss")}</Button>} />
      {lic.data && (
        <Alert>
          <AlertTitle>{t("license.status_title")}</AlertTitle>
          <AlertDescription>
            {t("license.status_text", { count: lic.data.active_clients, limit: lic.data.free_limit })}
            {lic.data.over_free_limit ? ` ${t("license.status_over")}` : ` ${t("license.status_ok")}`}
          </AlertDescription>
        </Alert>
      )}
      <div className="flex flex-col gap-3">
        {w.steps.map((s) => (
          <Card key={s.id}>
            <CardContent className="flex flex-col gap-3 pt-6 sm:flex-row sm:items-center">
              {s.done ? <CheckCircleIcon className="size-6 shrink-0 text-primary" weight="fill" /> : <CircleIcon className="size-6 shrink-0 text-muted-foreground" />}
              <div className="flex min-w-0 flex-1 flex-col gap-1">
                <p className="flex flex-wrap items-center gap-2 font-medium">{t(`setup.step.${s.id}` as TranslationKey)}{s.optional && <Badge variant="outline">{t("setup.optional")}</Badge>}</p>
                <p className="text-sm text-muted-foreground">{t(`setup.step.${s.id}_hint` as TranslationKey)}</p>
              </div>
              {s.id === "license" && !s.done ? (
                <Button type="button" size="sm" disabled={acceptLic.isPending} onClick={() => acceptLic.mutate()}>{t("license.accept")}</Button>
              ) : (
                <Button asChild variant={s.done ? "ghost" : "default"} size="sm"><Link to={s.href}>{s.done ? t("setup.review") : t("setup.configure")}</Link></Button>
              )}
            </CardContent>
          </Card>
        ))}
      </div>
    </div>
  );
}
