import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "react-router";
import { HeartIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { EmptyState } from "@/components/EmptyState";
import { LoadError } from "@/components/LoadError";
import { PageHeader } from "@/components/PageHeader";
import { ProductArt } from "@/components/store/ProductArt";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { api, ApiError } from "@/lib/api";
import type { WishlistItem } from "@/lib/creatorTools";
import { useI18n } from "@/lib/i18n";

/** Client: marketplace listings saved for later. */
export function WishlistPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["client", "wishlist"], queryFn: () => api<{ items: WishlistItem[] }>("/client/wishlist") });
  const remove = useMutation({
    mutationFn: (id: string) => api(`/client/wishlist/${id}`, { method: "DELETE" }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["client", "wishlist"] }),
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (q.isError && !q.data) return <LoadError retry={() => void q.refetch()} />;
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("ctools.wishlist_title")} description={t("ctools.wishlist_description")} />
      {!q.data ? <Skeleton className="h-48 w-full" /> : q.data.items.length === 0 ? (
        <EmptyState icon={HeartIcon} title={t("ctools.wishlist_empty")} description={t("ctools.wishlist_empty_hint")} action={<Button asChild variant="outline"><Link to="/marketplace">{t("pub.marketplace")}</Link></Button>} />
      ) : (
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3" data-testid="wishlist">
          {q.data.items.map((w) => (
            <Card key={w.product_id} className="overflow-hidden pt-0">
              <Link to={`/marketplace/r/${w.product_id}`} className="block aspect-video overflow-hidden border-b"><ProductArt src={w.image_url} name={w.name} /></Link>
              <CardContent className="flex items-start justify-between gap-2">
                <div className="min-w-0">
                  <Link to={`/marketplace/r/${w.product_id}`} className="block truncate font-medium hover:underline">{w.name}</Link>
                  {w.creator && <Link to={`/creators/${w.creator_slug}`} className="text-sm text-muted-foreground hover:text-foreground">{w.creator}</Link>}
                  {!w.available && <Badge variant="outline" className="mt-1">{t("ctools.unavailable")}</Badge>}
                </div>
                <Button size="icon" variant="ghost" aria-label={t("ctools.wish_remove")} disabled={remove.isPending} onClick={() => remove.mutate(w.product_id)}><TrashIcon aria-hidden /></Button>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
