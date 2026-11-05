import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircleIcon, CreditCardIcon, HourglassIcon, XCircleIcon } from "@phosphor-icons/react";
import { PageHeader } from "@/components/PageHeader";
import { PageSkeleton } from "@/components/PageSkeleton";
import { FormError } from "@/components/Field";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { api, ApiError } from "@/lib/api";
import { formatMoney } from "@/lib/billing/billing";
import { useCrumb } from "@/lib/crumb";
import { useI18n } from "@/lib/i18n";
import { BankInstructions } from "@/pages/client/billing/InvoiceDetail";

interface Option { id: string; kind: string; name: string }
interface Eligible { items: Option[]; currency: string; amount: string; count: number; invoice_ids: string[] }
type PayInit = { type: "redirect"; url: string } | { type: "instructions"; text: string; reference: string; amount: string; currency: string };
interface Bank { beneficiary: string; iban: string; bic: string; bank_name: string; note: string }
interface Outcome { status: string; invoice_id: string }

/** Parse `?ids=` into a stable, de-duplicated list (2-50). */
export function parsePayManyIds(raw: string | null): string[] {
  const ids = [...new Set((raw ?? "").split(",").map((s) => s.trim()).filter(Boolean))];
  ids.sort();
  return ids.length >= 2 && ids.length <= 50 ? ids : [];
}

/** One gateway checkout for several unpaid invoices (no merge). */
export function ClientPayManyPage() {
  const [params] = useSearchParams();
  const ids = useMemo(() => parsePayManyIds(params.get("ids")), [params]);
  const idsQ = ids.join(",");
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const [instructions, setInstructions] = useState<(Extract<PayInit, { type: "instructions" }> & { bank: Bank | null }) | null>(null);
  const { data } = useQuery({
    queryKey: ["client", "pay-many-gateways", idsQ],
    enabled: ids.length >= 2,
    queryFn: () => api<Eligible>(`/client/invoices/pay-many/gateways?ids=${encodeURIComponent(idsQ)}`),
  });
  useCrumb(t("client_pay.many_title"));
  const pay = useMutation({
    mutationFn: (gateway: string) => api<{ init: PayInit; bank: Bank | null }>("/client/invoices/pay-many", {
      method: "POST",
      body: { invoice_ids: ids, gateway },
    }),
    onSuccess: (r) => {
      setError(null);
      if (r.init.type === "redirect") window.location.assign(r.init.url);
      else setInstructions({ ...r.init, bank: r.bank });
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const paymentId = params.get("payment");
  const returning = params.get("return") === "1" && !!paymentId;
  const complete = useMutation({
    mutationFn: () => api<Outcome>(`/client/payments/${paymentId}/complete`, { method: "POST", body: { query: Object.fromEntries(params.entries()) } }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["client"] }),
  });
  const fired = useRef(false);
  useEffect(() => { if (returning && !fired.current) { fired.current = true; complete.mutate(); } }, [returning, complete]);

  const back = <Button asChild variant="outline" className="min-h-11 sm:min-h-9"><Link to="/invoices">{t("pay.back")}</Link></Button>;
  if (ids.length < 2) {
    return (
      <Card className="mx-auto w-full max-w-lg">
        <CardHeader><CardTitle>{t("client_pay.many_title")}</CardTitle><CardDescription>{tCode("error.merge_needs_two")}</CardDescription></CardHeader>
        <CardContent>{back}</CardContent>
      </Card>
    );
  }
  if (returning) {
    const st = complete.data?.status;
    const [Icon, title, hint] = complete.isPending || !complete.data && !complete.isError
      ? [HourglassIcon, t("pay.checking"), t("pay.checking_hint")]
      : complete.isError || st === "failed" ? [XCircleIcon, t("pay.failed"), t("pay.failed_hint")]
      : st === "succeeded" ? [CheckCircleIcon, t("pay.succeeded"), t("pay.succeeded_hint")]
      : [HourglassIcon, t("pay.pending"), t("pay.pending_hint")];
    return (
      <Card className="mx-auto w-full max-w-lg" data-testid="pay-many-outcome">
        <CardHeader className="items-center text-center"><Icon aria-hidden className="size-10" /><CardTitle>{title}</CardTitle><CardDescription>{hint}</CardDescription></CardHeader>
        <CardContent className="flex justify-center">{back}</CardContent>
      </Card>
    );
  }
  if (!data) return <PageSkeleton />;
  return (
    <div className="mx-auto flex w-full max-w-lg flex-col gap-6">
      <PageHeader
        title={t("client_pay.many_title")}
        description={
          <>
            {t("client_pay.many_n", { n: data.count })} · {t("pay.amount_due")}:{" "}
            <span className="font-semibold tabular-nums text-foreground">{formatMoney(data.amount, data.currency, lang)}</span>
          </>
        }
      />
      <Card>
        <CardContent className="flex flex-col gap-3">
          <p className="text-sm text-muted-foreground">{t("client_pay.card_hint")}</p>
          {params.get("cancelled") === "1" && <p className="rounded-md border bg-muted p-3 text-sm">{t("pay.cancelled")}</p>}
          <FormError message={error} />
          {data.items.length === 0 ? (
            <p className="text-sm text-muted-foreground" data-testid="no-gateways">{t("billing.no_gateways")}</p>
          ) : (
            <div className="flex flex-col gap-2 [&_button]:min-h-11 sm:[&_button]:min-h-9" data-testid="gateway-options">
              {data.items.map((g, i) => (
                <Button key={g.id} variant={i === 0 ? "default" : "outline"} className="justify-start" disabled={pay.isPending} onClick={() => pay.mutate(g.id)}>
                  <CreditCardIcon aria-hidden />{g.name}
                </Button>
              ))}
            </div>
          )}
          {instructions && <BankInstructions i={instructions} />}
          <div>{back}</div>
        </CardContent>
      </Card>
    </div>
  );
}
