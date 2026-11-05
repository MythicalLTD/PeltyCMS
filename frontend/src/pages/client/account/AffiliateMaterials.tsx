import { CopyIcon, TrophyIcon } from "@phosphor-icons/react";
import { BlockSkeleton } from "@/components/PageSkeleton";
import { useQuery } from "@tanstack/react-query";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/lib/api";
import { materialSnippet, nextTier, type Material, type Tier } from "@/lib/admin/affiliateExtras";
import { useI18n } from "@/lib/i18n";

interface Resp { code: string; effective_percent: string; tiers: Tier[]; items: Material[] }

/** Tier progress + copy-ready promo materials with the referral link. */
export function AffiliateMaterials({ url, referrals }: { url: string; referrals: number }) {
  const { t } = useI18n();
  const { data } = useQuery({ queryKey: ["client", "affiliate", "materials"], queryFn: () => api<Resp>("/client/affiliate/materials") });
  if (!data) return <BlockSkeleton />;
  const next = nextTier(data.tiers, referrals);
  const copy = (s: string) => { void navigator.clipboard?.writeText(s).then(() => toast.success(t("affiliate.copied"))); };
  return (
    <>
      {data.tiers.length > 0 && (
        <Card>
          <CardHeader><CardTitle className="flex items-center gap-2"><TrophyIcon aria-hidden />{t("affiliate.your_rate", { rate: Number(data.effective_percent) })}</CardTitle>
            <CardDescription>{next ? t("affiliate.next_tier", { name: next.name, n: next.min_referrals - referrals, rate: Number(next.commission_percent) }) : t("affiliate.top_tier")}</CardDescription>
          </CardHeader>
        </Card>
      )}
      {data.items.length > 0 && (
        <Card>
          <CardHeader><CardTitle>{t("affiliate.materials")}</CardTitle><CardDescription>{t("affiliate.materials_client_help")}</CardDescription></CardHeader>
          <CardContent className="grid gap-4 md:grid-cols-2">
            {data.items.map((m) => {
              const snip = materialSnippet(m, url);
              return (
                <div key={m.id} className="flex min-w-0 flex-col gap-2 rounded-md border p-3">
                  <span className="font-medium">{m.title}</span>
                  {m.kind === "banner" && <img src={m.image_url} alt={m.title} className="max-h-32 max-w-full self-start object-contain" loading="lazy" />}
                  <Textarea readOnly rows={3} value={snip} className="font-mono text-xs" aria-label={m.title} />
                  <Button size="sm" variant="outline" className="self-start" onClick={() => copy(snip)}><CopyIcon aria-hidden />{t("affiliate.copy_code")}</Button>
                </div>
              );
            })}
          </CardContent>
        </Card>
      )}
    </>
  );
}
