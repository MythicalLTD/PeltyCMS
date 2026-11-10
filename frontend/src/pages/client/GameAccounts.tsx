import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { api, ApiError } from "@/lib/api";
import type { GameAccount, GameDef, GameDelivery } from "@/lib/gaming";
import { useModules } from "@/lib/modules";
import { Badge } from "@/components/ui/badge";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";

/** Client: link game identities so purchases can be delivered in-game. */
export function GameAccountsPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const games = useQuery({ queryKey: ["public", "games"], queryFn: () => api<{ items: GameDef[] }>("/public/game/games") });
  const mine = useQuery({ queryKey: ["client", "game-accounts"], queryFn: () => api<{ items: GameAccount[] }>("/client/game/accounts") });
  const [draft, setDraft] = useState<Record<string, string>>({});
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const refresh = () => void qc.invalidateQueries({ queryKey: ["client", "game-accounts"] });
  const link = useMutation({
    mutationFn: (b: { game: string; field: string; value: string }) => api<GameAccount>("/client/game/accounts", { method: "POST", body: b }),
    onSuccess: (a) => { setDraft((d) => ({ ...d, [`${a.game}:${a.field}`]: "" })); refresh(); toast.success(t("game.accounts.linked")); },
    onError: fail,
  });
  const unlink = useMutation({ mutationFn: (id: string) => api(`/client/game/accounts/${id}`, { method: "DELETE" }), onSuccess: refresh, onError: fail });
  const { enabled } = useModules();
  const delivery = enabled.has("game_delivery");
  const deliveries = useQuery({ queryKey: ["client", "game-deliveries"], enabled: delivery, queryFn: () => api<{ items: GameDelivery[] }>("/client/game/deliveries") });
  if (!games.data || !mine.data) return <Skeleton className="h-48 w-full" />;
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("game.accounts.title")} description={t("game.accounts.description")} />
      <div className="grid gap-3 md:grid-cols-2">
        {games.data.items.map((g) => (
          <Card key={g.id} data-game={g.id}>
            <CardHeader><CardTitle className="text-base">{g.name}</CardTitle></CardHeader>
            <CardContent className="flex flex-col gap-3">
              {g.fields.map((f) => {
                const have = mine.data.items.find((a) => a.game === g.id && a.field === f);
                const key = `${g.id}:${f}`;
                return (
                  <form key={f} className="flex flex-col gap-1" onSubmit={(e) => { e.preventDefault(); link.mutate({ game: g.id, field: f, value: draft[key] ?? "" }); }}>
                    <Label htmlFor={`ga-${key}`}>{t(`game.field.${f}` as TranslationKey)}</Label>
                    <div className="flex gap-2">
                      <Input id={`ga-${key}`} maxLength={100} placeholder={have?.value ?? ""} value={draft[key] ?? ""} onChange={(e) => setDraft((d) => ({ ...d, [key]: e.target.value }))} />
                      <Button type="submit" variant="outline" disabled={!(draft[key] ?? "").trim() || link.isPending}>{have ? t("common.save") : t("game.accounts.link")}</Button>
                      {have && <Button type="button" variant="ghost" aria-label={t("common.delete")} onClick={() => unlink.mutate(have.id)}><TrashIcon aria-hidden /></Button>}
                    </div>
                  </form>
                );
              })}
            </CardContent>
          </Card>
        ))}
      </div>
      {delivery && (
        <Card data-testid="my-deliveries">
          <CardHeader><CardTitle className="text-base">{t("game.my_deliveries.title")}</CardTitle></CardHeader>
          <CardContent className="flex flex-col gap-2 text-sm">
            {(deliveries.data?.items ?? []).length === 0 && <p className="text-muted-foreground">{t("game.my_deliveries.empty")}</p>}
            {(deliveries.data?.items ?? []).map((d) => (
              <div key={d.id} className="flex flex-wrap items-center justify-between gap-2 border-b pb-2 last:border-0">
                <span>{d.package ?? "—"} · {d.server_name}</span>
                <Badge variant={d.status === "delivered" ? "secondary" : d.status === "failed" ? "destructive" : "outline"}>{t(`game.delivery.st.${d.status}` as TranslationKey)}</Badge>
              </div>
            ))}
            {(deliveries.data?.items ?? []).some((d) => d.status === "waiting_identity") && <output className="text-muted-foreground">{t("game.my_deliveries.waiting_hint")}</output>}
          </CardContent>
        </Card>
      )}
    </div>
  );
}
