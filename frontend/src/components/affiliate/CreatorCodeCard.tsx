import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CopyIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { FormError } from "@/components/Field";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, ApiError } from "@/lib/api";
import { normalizeCreatorCode, type CreatorCode } from "@/lib/creatorCodes";
import { useI18n } from "@/lib/i18n";

/** Affiliate dashboard card: pick the creator code buyers type at checkout. */
export function CreatorCodeCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const key = ["client", "creator-code"];
  const q = useQuery({ queryKey: key, queryFn: () => api<{ code: CreatorCode | null; orders: number }>("/client/creator-code") });
  const [draft, setDraft] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: (code: string) => api<CreatorCode>("/client/creator-code", { method: "PUT", body: { code } }),
    onSuccess: () => { setError(null); setDraft(null); toast.success(t("creator_code.saved")); void qc.invalidateQueries({ queryKey: key }); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (!q.data) return null;
  const current = q.data.code;
  const value = draft ?? current?.code ?? "";
  const valid = normalizeCreatorCode(value);
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("creator_code.title")}</CardTitle>
        <CardDescription>
          {current && Number(current.discount_percent) > 0 ? t("creator_code.hint_discount", { n: Number(current.discount_percent) }) : t("creator_code.hint")}
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-3">
        <form className="flex flex-col gap-2 sm:flex-row sm:items-end" onSubmit={(e) => { e.preventDefault(); if (valid) save.mutate(valid); }}>
          <div className="grid flex-1 gap-2">
            <Label htmlFor="creator-code">{t("creator_code.code")}</Label>
            <Input id="creator-code" className="font-mono" maxLength={24} value={value} onChange={(e) => setDraft(e.target.value)} aria-invalid={value !== "" && !valid} />
          </div>
          <Button type="submit" disabled={save.isPending || !valid || valid === current?.code}>{t("common.save")}</Button>
          {current && <Button type="button" variant="outline" onClick={() => { void navigator.clipboard.writeText(current.code); toast.success(t("affiliate.copied")); }}><CopyIcon aria-hidden />{t("affiliate.copy")}</Button>}
        </form>
        <p className="text-xs text-muted-foreground">{t("creator_code.rules")}</p>
        {current && <p className="text-sm">{t("creator_code.orders", { n: q.data.orders })}{current.enabled ? "" : ` · ${t("creator_code.disabled")}`}</p>}
        <FormError message={error} />
      </CardContent>
    </Card>
  );
}
