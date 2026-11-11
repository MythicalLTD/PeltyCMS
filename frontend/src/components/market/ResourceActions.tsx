import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useLocation, useNavigate } from "react-router";
import { ChatCircleDotsIcon, HeartIcon, PackageIcon, ShoppingCartIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Field } from "@/components/Field";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { useMe } from "@/lib/auth";
import { formatMoney } from "@/lib/billing/billing";
import { bundlePrice, type CreatorBundle, type WishlistItem } from "@/lib/creatorTools";
import { useI18n } from "@/lib/i18n";
import { useStoreCart, type StoreLine } from "@/lib/store/storefront";
import { useCatalog, useStoreCurrency } from "@/pages/store/Store";

function useLoginFirst() {
  const nav = useNavigate();
  const loc = useLocation();
  return () => nav("/login", { state: { from: loc.pathname + loc.search } });
}

/** Heart toggle: save a listing to the signed-in customer's wishlist. */
export function WishlistButton({ productId }: { productId: string }) {
  const { t, tCode } = useI18n();
  const { data: me } = useMe();
  const qc = useQueryClient();
  const login = useLoginFirst();
  const list = useQuery({ queryKey: ["client", "wishlist"], queryFn: () => api<{ items: WishlistItem[] }>("/client/wishlist"), enabled: !!me });
  const saved = !!list.data?.items.some((i) => i.product_id === productId);
  const m = useMutation({
    mutationFn: () => api(`/client/wishlist/${productId}`, { method: saved ? "DELETE" : "PUT" }),
    onSuccess: () => { void qc.invalidateQueries({ queryKey: ["client", "wishlist"] }); toast.success(saved ? t("ctools.wish_removed") : t("ctools.wish_added")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Button variant="outline" aria-pressed={saved} data-testid="wishlist-toggle" disabled={m.isPending} onClick={() => (me ? m.mutate() : login())}>
      <HeartIcon aria-hidden weight={saved ? "fill" : "regular"} className={saved ? "text-destructive" : undefined} />
      {saved ? t("ctools.wishlisted") : t("ctools.wishlist_add")}
    </Button>
  );
}

/** Ask the creator a question; opens a conversation in /messages. */
export function MessageCreator({ productId, product, creator }: { productId: string; product: string; creator: string }) {
  const { t, tCode } = useI18n();
  const { data: me } = useMe();
  const nav = useNavigate();
  const login = useLoginFirst();
  const [open, setOpen] = useState(false);
  const [subject, setSubject] = useState("");
  const [body, setBody] = useState("");
  const send = useMutation({
    mutationFn: () => api<{ id: string }>("/client/market-messages", { method: "POST", body: { product_id: productId, subject: subject.trim() || product.slice(0, 150), body: body.trim() } }),
    onSuccess: (c) => { setOpen(false); toast.success(t("ctools.message_sent")); nav(`/messages?c=${c.id}`); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <>
      <Button variant="outline" data-testid="message-creator" onClick={() => (me ? setOpen(true) : login())}><ChatCircleDotsIcon aria-hidden />{t("ctools.message_creator")}</Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("ctools.message_title", { name: creator })}</DialogTitle><DialogDescription>{t("ctools.message_hint")}</DialogDescription></DialogHeader>
          <form id="msg-new" className="grid gap-3" onSubmit={(e) => { e.preventDefault(); send.mutate(); }}>
            <Field id="msg-subject" label={t("ctools.subject")} maxLength={150} placeholder={product} value={subject} onChange={(e) => setSubject(e.target.value)} />
            <div className="grid gap-2"><Label htmlFor="msg-body">{t("ctools.message")}</Label><Textarea id="msg-body" rows={5} maxLength={5000} required value={body} onChange={(e) => setBody(e.target.value)} /></div>
          </form>
          <DialogFooter><Button type="submit" form="msg-new" disabled={!body.trim() || send.isPending}>{t("ctools.send")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

/** "Frequently bought together": creator bundles that include this listing. */
export function BundleOffers({ bundles }: { bundles: CreatorBundle[] }) {
  const { t, tCode, lang } = useI18n();
  const { data } = useCatalog();
  const { currency } = useStoreCurrency();
  const { view, save } = useStoreCart();
  const nav = useNavigate();
  if (bundles.length === 0) return null;
  const priceOf = (id: string, cycle: string) => Number(data?.products.find((p) => p.id === id)?.prices.find((p) => p.currency === currency && p.cycle === cycle)?.amount ?? NaN);
  const add = (b: CreatorBundle) => {
    const cart = view && !view.cart.order_id ? view.cart : null;
    const base = cart ?? { items: [] as StoreLine[], country: "", coupon: null, email: null };
    const missing = b.items.filter((i) => !base.items.some((l) => l.product_id === i.product_id)).map((i) => ({ product_id: i.product_id, cycle: i.cycle, config: {}, addons: [] }));
    save.mutate({ ...base, currency, items: [...base.items, ...missing], locale: lang }, {
      onSuccess: () => nav("/cart"),
      onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
    });
  };
  return (
    <>
      {bundles.map((b) => {
        const prices = b.items.map((i) => priceOf(i.product_id, i.cycle));
        const known = prices.every((p) => Number.isFinite(p));
        const pr = bundlePrice(known ? prices : [], Number(b.discount_percent));
        return (
          <Card key={b.id} data-testid="bundle-offer">
            <CardHeader>
              <CardTitle className="flex items-center justify-between gap-2 text-base"><span className="flex items-center gap-1.5"><PackageIcon aria-hidden />{b.name}</span><Badge>{t("ctools.off_percent", { n: Number(b.discount_percent) })}</Badge></CardTitle>
              {b.description && <CardDescription>{b.description}</CardDescription>}
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <ul className="flex flex-col gap-1 text-sm">{b.items.map((i) => <li key={i.product_id} className="truncate">{i.product_name}</li>)}</ul>
              {known && <p className="text-sm"><span className="mr-2 text-muted-foreground line-through tabular-nums">{formatMoney(String(pr.full), currency, lang)}</span><span className="font-semibold tabular-nums">{formatMoney(String(pr.price), currency, lang)}</span></p>}
              <Button disabled={save.isPending} onClick={() => add(b)}><ShoppingCartIcon aria-hidden />{t("ctools.add_bundle")}</Button>
            </CardContent>
          </Card>
        );
      })}
    </>
  );
}
