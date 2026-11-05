import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

interface Tos { id: string; version: string; url: string; summary: string; published_at: string }
interface MyTos { current: Tos | null; accepted_at: string | null; pending: boolean; history: { version: string; accepted_at: string }[] }

/** Terms of Service acceptance (GDPR consent record). Marketing consent lives in the existing marketing preference card. */
export function ConsentCard() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const onErr = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const tos = useQuery({ queryKey: ["account", "tos"], queryFn: () => api<MyTos>("/account/tos") });
  const accept = useMutation({
    mutationFn: (id: string) => api("/account/tos/accept", { method: "POST", body: { version_id: id } }),
    onSuccess: () => { toast.success(t("consent.accepted")); void qc.invalidateQueries({ queryKey: ["account", "tos"] }); },
    onError: onErr,
  });
  const cur = tos.data?.current;
  if (!cur) return null;
  return (
    <Card>
      <CardHeader><CardTitle>{t("consent.title")}</CardTitle><CardDescription>{t("consent.hint")}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-4">
        {(
          <div className="flex flex-col gap-2">
            {tos.data?.pending ? (
              <Alert>
                <AlertDescription className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                  <span>{t("consent.tos_pending", { version: cur.version })}{cur.summary ? `, ${cur.summary}` : ""}</span>
                  <div className="flex flex-wrap gap-2">
                    {cur.url && <Button variant="outline" size="sm" asChild><a href={cur.url} target="_blank" rel="noreferrer">{t("consent.read")}</a></Button>}
                    <Button size="sm" onClick={() => accept.mutate(cur.id)} disabled={accept.isPending}>{t("consent.accept")}</Button>
                  </div>
                </AlertDescription>
              </Alert>
            ) : (
              <p className="text-sm text-muted-foreground">
                {t("consent.tos_accepted", { version: cur.version, date: tos.data?.accepted_at ? formatDateTime(tos.data.accepted_at, lang) : "-" })}
                {cur.url && <> · <a className="text-primary hover:underline" href={cur.url} target="_blank" rel="noreferrer">{t("consent.read")}</a></>}
              </p>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
