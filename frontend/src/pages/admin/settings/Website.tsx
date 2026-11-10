import { useId, useState } from "react";
import { PageHeader } from "@/components/PageHeader";
import { PageSkeleton } from "@/components/PageSkeleton";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  ArrowDownIcon, ArrowSquareOutIcon, ArrowUpIcon, FloppyDiskIcon, PencilSimpleIcon, PlusIcon, RocketLaunchIcon, TrashIcon,
} from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { ImageUrlField, isImageFieldName } from "@/components/admin/ImageUrlField";
import { JOIN_CODE, parseJoinCode } from "@/lib/store/game";
import { PUBLIC_MODES, resolveMode, type PublicModeSetting } from "@/lib/publicMode";
import { useModules } from "@/lib/modules";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import {
  BLOCK_TYPES, FONTS, moveBlock, newBlock, normalizeConfig, normalizePages, productLanding, SOCIAL_NETWORKS,
  type Block, type FooterColumn, type SiteConfig, type SiteLink, type SitePage,
} from "@/lib/admin/site";

const LONG = new Set(["subtitle", "text", "body", "a", "caption"]);

function useErr() {
  const { tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : String(e));
}

function LinksEditor({ value, onChange }: { value: SiteLink[]; onChange: (v: SiteLink[]) => void }) {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-2">
      {value.map((l, i) => (
        <div key={i} className="flex flex-col gap-2 sm:flex-row">
          <Input aria-label={t("site.admin.label")} placeholder={t("site.admin.label")} value={l.label} onChange={(e) => onChange(value.map((x, j) => (j === i ? { ...x, label: e.target.value } : x)))} />
          <Input aria-label={t("site.admin.href")} placeholder="/store" value={l.href} onChange={(e) => onChange(value.map((x, j) => (j === i ? { ...x, href: e.target.value } : x)))} />
          <div className="flex gap-1">
            <Button type="button" variant="outline" size="icon" className="size-11 md:size-9" disabled={i === 0} aria-label={t("site.admin.up")} onClick={() => onChange(moveBlock(value, i, i - 1))}><ArrowUpIcon /></Button>
            <Button type="button" variant="outline" size="icon" className="size-11 md:size-9" disabled={i === value.length - 1} aria-label={t("site.admin.down")} onClick={() => onChange(moveBlock(value, i, i + 1))}><ArrowDownIcon /></Button>
            <Button type="button" variant="outline" size="icon" className="size-11 md:size-9" aria-label={t("common.delete")} onClick={() => onChange(value.filter((_, j) => j !== i))}><TrashIcon /></Button>
          </div>
        </div>
      ))}
      <Button type="button" variant="outline" size="sm" className="self-start" onClick={() => onChange([...value, { label: "", href: "/" }])}><PlusIcon />{t("site.admin.add_link")}</Button>
    </div>
  );
}

function SettingsTab() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const err = useErr();
  const q = useQuery({ queryKey: ["admin", "site"], queryFn: async () => normalizeConfig(await api<unknown>("/admin/site")) });
  const [c, setC] = useState<SiteConfig | null>(q.data ?? null);
  const [seen, setSeen] = useState(q.data);
  if (q.data !== seen) {
    setSeen(q.data);
    if (q.data !== undefined) setC(q.data);
  }
  const save = useMutation({
    mutationFn: async (b: SiteConfig) => normalizeConfig(await api<unknown>("/admin/site", { method: "PUT", body: b })),
    onSuccess: (d) => { qc.setQueryData(["admin", "site"], d); qc.invalidateQueries({ queryKey: ["public", "site"] }); toast.success(t("common.saved")); },
    onError: err,
  });
  if (!c) return <PageSkeleton />;
  const set = <K extends keyof SiteConfig>(k: K, v: SiteConfig[K]) => setC({ ...c, [k]: v });
  const cols = c.footer_columns;
  const setCol = (i: number, v: FooterColumn) => set("footer_columns", cols.map((x, j) => (j === i ? v : x)));
  return (
    <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(c); }}>
      <Card>
        <CardHeader><CardTitle>{t("site.admin.brand")}</CardTitle><CardDescription>{t("site.admin.brand_hint")}</CardDescription></CardHeader>
        <CardContent className="grid gap-4 sm:grid-cols-2">
          <div className="flex flex-col gap-2"><Label htmlFor="s-name">{t("site.admin.site_name")}</Label><Input id="s-name" value={c.site_name} onChange={(e) => set("site_name", e.target.value)} /></div>
          <div className="flex flex-col gap-2"><Label htmlFor="s-tag">{t("site.admin.tagline")}</Label><Input id="s-tag" value={c.tagline} onChange={(e) => set("tagline", e.target.value)} /></div>
          <ImageUrlField id="s-logo" label={t("site.admin.logo_url")} value={c.logo_url} onChange={(url) => set("logo_url", url)} preferPath placeholder="/brand/logo.png" hint={t("media.hint")} />
          <div className="flex flex-col gap-2">
            <Label htmlFor="s-color">{t("site.admin.primary_color")}</Label>
            <div className="flex gap-2">
              <Input id="s-color-pick" type="color" aria-label={t("site.admin.primary_color")} className="w-14 p-1" value={/^#[0-9a-f]{6}$/i.test(c.primary_color) ? c.primary_color : "#59b5d3"} onChange={(e) => set("primary_color", e.target.value)} />
              <Input id="s-color" value={c.primary_color} placeholder={`#59b5d3 (${t("site.admin.theme_default")})`} onChange={(e) => set("primary_color", e.target.value)} />
            </div>
          </div>
          <div className="flex flex-col gap-2">
            <Label>{t("site.admin.font")}</Label>
            <Select value={c.font} onValueChange={(v) => set("font", v as SiteConfig["font"])}>
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>{Object.keys(FONTS).map((f) => <SelectItem key={f} value={f}>{f}</SelectItem>)}</SelectContent>
            </Select>
          </div>
          <div className="flex flex-col gap-2">
            <Label>{t("site.admin.default_mode")}</Label>
            <Select value={c.default_mode} onValueChange={(v) => set("default_mode", v as SiteConfig["default_mode"])}>
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="dark">{t("site.admin.mode_dark")}</SelectItem>
                <SelectItem value="light">{t("site.admin.mode_light")}</SelectItem>
                <SelectItem value="system">{t("site.admin.mode_system")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="flex flex-col gap-2"><Label htmlFor="s-home">{t("site.admin.homepage")}</Label><Input id="s-home" value={c.homepage} onChange={(e) => set("homepage", e.target.value)} /></div>
          <div className="flex items-center gap-3 self-end py-2"><Switch id="s-store" checked={c.show_store_in_nav} onCheckedChange={(v) => set("show_store_in_nav", v)} /><Label htmlFor="s-store">{t("site.admin.show_store")}</Label></div>
        </CardContent>
      </Card>
      <Card>
        <CardHeader><CardTitle>{t("site.admin.store")}</CardTitle><CardDescription>{t("site.admin.store_hint")}</CardDescription></CardHeader>
        <CardContent className="grid gap-4 sm:grid-cols-2">
          <div className="flex flex-col gap-2">
            <Label htmlFor="s-store-mode">{t("site.admin.store_mode")}</Label>
            <Select value={c.store_mode} onValueChange={(v) => set("store_mode", v as SiteConfig["store_mode"])}>
              <SelectTrigger id="s-store-mode" className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="hosting">{t("site.admin.store_mode_hosting")}</SelectItem>
                <SelectItem value="game">{t("site.admin.store_mode_game")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <PublicModeField value={c.public_mode} onChange={(v) => set("public_mode", v)} />
          {c.store_mode === "game" && (
            <div className="flex flex-col gap-2">
              <Label htmlFor="s-join">{t("site.admin.game_join_code")}</Label>
              <Input id="s-join" value={c.game_join_code} placeholder="abc123" autoCapitalize="none" spellCheck={false} aria-describedby="s-join-hint"
                aria-invalid={c.game_join_code !== "" && !JOIN_CODE.test(c.game_join_code)}
                onChange={(e) => set("game_join_code", parseJoinCode(e.target.value))} />
              <p id="s-join-hint" className="text-xs text-muted-foreground">{t("site.admin.game_join_code_hint")}</p>
            </div>
          )}
        </CardContent>
      </Card>
      <Card>
        <CardHeader><CardTitle>{t("site.admin.nav")}</CardTitle><CardDescription>{t("site.admin.nav_hint")}</CardDescription></CardHeader>
        <CardContent><LinksEditor value={c.nav} onChange={(v) => set("nav", v)} /></CardContent>
      </Card>
      <Card>
        <CardHeader><CardTitle>{t("site.admin.footer")}</CardTitle></CardHeader>
        <CardContent className="flex flex-col gap-4">
          {cols.map((col, i) => (
            <div key={i} className="flex flex-col gap-2 rounded-md border p-3">
              <div className="flex gap-2">
                <Input aria-label={t("site.admin.column_title")} placeholder={t("site.admin.column_title")} value={col.title} onChange={(e) => setCol(i, { ...col, title: e.target.value })} />
                <Button type="button" variant="outline" size="icon" className="size-11 md:size-9" aria-label={t("common.delete")} onClick={() => set("footer_columns", cols.filter((_, j) => j !== i))}><TrashIcon /></Button>
              </div>
              <LinksEditor value={col.links} onChange={(v) => setCol(i, { ...col, links: v })} />
            </div>
          ))}
          <Button type="button" variant="outline" size="sm" className="self-start" onClick={() => set("footer_columns", [...cols, { title: "", links: [] }])}><PlusIcon />{t("site.admin.add_column")}</Button>
          <div className="flex flex-col gap-2"><Label htmlFor="s-ft">{t("site.admin.footer_text")}</Label><Input id="s-ft" value={c.footer_text} onChange={(e) => set("footer_text", e.target.value)} /></div>
          <div className="flex flex-col gap-2"><Label htmlFor="s-copy">{t("site.admin.copyright")}</Label><Input id="s-copy" placeholder={t("site.admin.copyright_hint")} value={c.copyright} onChange={(e) => set("copyright", e.target.value)} /></div>
          <div className="flex flex-col gap-2">
            <Label>{t("site.admin.social")}</Label>
            {c.social.map((x, i) => (
              <div key={i} className="flex flex-col gap-2 sm:flex-row">
                <Select value={x.network} onValueChange={(v) => set("social", c.social.map((y, j) => (j === i ? { ...y, network: v } : y)))}>
                  <SelectTrigger className="w-full sm:w-40" aria-label={t("site.admin.network")}><SelectValue /></SelectTrigger>
                  <SelectContent>{SOCIAL_NETWORKS.map((n) => <SelectItem key={n} value={n}>{n}</SelectItem>)}</SelectContent>
                </Select>
                <Input aria-label={t("site.admin.href")} placeholder="https://" value={x.href} onChange={(e) => set("social", c.social.map((y, j) => (j === i ? { ...y, href: e.target.value } : y)))} />
                <Button type="button" variant="outline" size="icon" className="size-11 md:size-9" aria-label={t("common.delete")} onClick={() => set("social", c.social.filter((_, j) => j !== i))}><TrashIcon /></Button>
              </div>
            ))}
            <Button type="button" variant="outline" size="sm" className="self-start" onClick={() => set("social", [...c.social, { network: "github", href: "https://" }])}><PlusIcon />{t("site.admin.add_social")}</Button>
          </div>
        </CardContent>
      </Card>
      <Button type="submit" className="self-end" disabled={save.isPending}><FloppyDiskIcon />{t("common.save")}</Button>
    </form>
  );
}

type Row = Record<string, unknown>;

function FieldInput({ name, value, onChange }: { name: string; value: unknown; onChange: (v: unknown) => void }) {
  const id = useId();
  if (typeof value === "boolean") return <div className="flex items-center gap-2"><Switch id={id} checked={value} onCheckedChange={onChange} /><Label htmlFor={id}>{name}</Label></div>;
  if (typeof value === "string" && isImageFieldName(name)) {
    return (
      <ImageUrlField
        id={id}
        label={name}
        value={value}
        onChange={onChange}
        preferPath
        className="sm:col-span-2"
      />
    );
  }
  const isNum = typeof value === "number";
  const Comp = LONG.has(name) ? Textarea : Input;
  return (
    <div className="flex flex-col gap-1">
      <Label htmlFor={id} className="text-xs text-muted-foreground">{name}</Label>
      <Comp id={id} type={isNum ? "number" : undefined} value={String(value ?? "")} onChange={(e: React.ChangeEvent<HTMLInputElement & HTMLTextAreaElement>) => onChange(isNum ? Number(e.target.value) : e.target.value)} />
    </div>
  );
}

interface CatalogLite { products: { id: string; name: string; prices: { currency: string; cycle: string; amount: string }[] }[] }

/** Ordered product picker for the products block (order = card order). */
function ProductPicker({ value, onChange }: { value: string[]; onChange: (v: string[]) => void }) {
  const { t } = useI18n();
  const q = useQuery({ queryKey: ["catalog"], queryFn: () => api<CatalogLite>("/catalog") });
  const all = q.data?.products ?? [];
  return (
    <div className="flex flex-col gap-2" data-testid="product-picker">
      <Label>{t("site.admin.products_pick")}</Label>
      <div className="flex flex-wrap gap-2">
        {value.map((id, i) => {
          const pr = all.find((x) => x.id === id);
          return (
            <Badge key={id} variant="secondary" className="gap-1 py-1">
              {i + 1}. {pr?.name ?? id}
              <button type="button" aria-label={t("site.admin.up")} disabled={i === 0} onClick={() => onChange(moveBlock(value, i, i - 1))}><ArrowUpIcon size={12} /></button>
              <button type="button" aria-label={t("common.delete")} onClick={() => onChange(value.filter((x) => x !== id))}><TrashIcon size={12} /></button>
            </Badge>
          );
        })}
      </div>
      <div className="grid max-h-40 gap-1 overflow-y-auto rounded-md border p-2 sm:grid-cols-2">
        {all.filter((x) => !value.includes(x.id)).map((x) => (
          <button key={x.id} type="button" className="flex items-center gap-2 rounded px-2 py-1 text-left text-sm hover:bg-muted" onClick={() => onChange([...value, x.id])}><PlusIcon size={14} />{x.name}</button>
        ))}
      </div>
    </div>
  );
}

function BlockEditor({ block, onChange }: { block: Block; onChange: (b: Block) => void }) {
  const { t } = useI18n();
  const p = block.props;
  const setP = (k: string, v: unknown) => onChange({ ...block, props: { ...p, [k]: v } });
  return (
    <div className="flex flex-col gap-3">
      {block.type === "products" && <ProductPicker value={Array.isArray(p.product_ids) ? (p.product_ids as string[]) : []} onChange={(v) => setP("product_ids", v)} />}
      <div className="grid gap-3 sm:grid-cols-2">
        {Object.entries(p).filter(([, v]) => !Array.isArray(v)).map(([k, v]) => <FieldInput key={k} name={k} value={v} onChange={(nv) => setP(k, nv)} />)}
      </div>
      {Object.entries(p).filter(([k, v]) => Array.isArray(v) && k !== "product_ids").map(([k, v]) => {
        const rows = v as Row[];
        const template: Row = Object.fromEntries(Object.keys(rows[0] ?? { title: "" }).map((x) => [x, ""]));
        return (
          <div key={k} className="flex flex-col gap-2">
            <p className="text-sm font-medium">{k}</p>
            {rows.map((r, i) => (
              <div key={i} className="flex flex-col gap-2 rounded-md border p-3">
                <div className="grid gap-2 sm:grid-cols-2">
                  {Object.entries(r).map(([rk, rv]) => <FieldInput key={rk} name={rk} value={rv} onChange={(nv) => setP(k, rows.map((x, j) => (j === i ? { ...x, [rk]: nv } : x)))} />)}
                </div>
                <div className="flex gap-1 self-end">
                  <Button type="button" variant="outline" size="icon" className="size-11 md:size-9" disabled={i === 0} aria-label={t("site.admin.up")} onClick={() => setP(k, moveBlock(rows, i, i - 1))}><ArrowUpIcon /></Button>
                  <Button type="button" variant="outline" size="icon" className="size-11 md:size-9" disabled={i === rows.length - 1} aria-label={t("site.admin.down")} onClick={() => setP(k, moveBlock(rows, i, i + 1))}><ArrowDownIcon /></Button>
                  <Button type="button" variant="outline" size="icon" className="size-11 md:size-9" aria-label={t("common.delete")} onClick={() => setP(k, rows.filter((_, j) => j !== i))}><TrashIcon /></Button>
                </div>
              </div>
            ))}
            <Button type="button" variant="outline" size="sm" className="self-start" onClick={() => setP(k, [...rows, template])}><PlusIcon />{t("site.admin.add_item")}</Button>
          </div>
        );
      })}
    </div>
  );
}

function PageEditor({ page, onDone }: { page: SitePage | null; onDone: () => void }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const err = useErr();
  const [p, setP] = useState<Omit<SitePage, "id">>(page ?? { slug: "", title: "", description: "", blocks: [], published: false, show_in_nav: false, sort_order: 0 });
  const [open, setOpen] = useState<number | null>(null);
  const save = useMutation({
    mutationFn: () => api<SitePage>(page ? `/admin/site/pages/${page.id}` : "/admin/site/pages", { method: page ? "PUT" : "POST", body: p }),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ["admin", "site", "pages"] }); qc.invalidateQueries({ queryKey: ["public"] }); toast.success(t("common.saved")); onDone(); },
    onError: err,
  });
  const setB = (b: Block[]) => setP({ ...p, blocks: b });
  return (
    <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
      <Card>
        <CardHeader><CardTitle>{page ? t("site.admin.edit_page") : t("site.admin.new_page")}</CardTitle></CardHeader>
        <CardContent className="grid gap-4 sm:grid-cols-2">
          <div className="flex flex-col gap-2"><Label htmlFor="p-title">{t("site.admin.page_title")}</Label><Input id="p-title" required value={p.title} onChange={(e) => setP({ ...p, title: e.target.value })} /></div>
          <div className="flex flex-col gap-2"><Label htmlFor="p-slug">{t("site.admin.slug")}</Label><Input id="p-slug" required pattern="[a-z0-9-]+" value={p.slug} onChange={(e) => setP({ ...p, slug: e.target.value })} /></div>
          <div className="flex flex-col gap-2 sm:col-span-2"><Label htmlFor="p-desc">{t("site.admin.description")}</Label><Input id="p-desc" value={p.description} onChange={(e) => setP({ ...p, description: e.target.value })} /></div>
          <div className="flex flex-col gap-2"><Label htmlFor="p-order">{t("site.admin.sort_order")}</Label><Input id="p-order" type="number" value={p.sort_order} onChange={(e) => setP({ ...p, sort_order: Number(e.target.value) })} /></div>
          <div className="flex flex-wrap items-center gap-6 self-end py-2">
            <div className="flex items-center gap-2"><Switch id="p-pub" checked={p.published} onCheckedChange={(v) => setP({ ...p, published: v })} /><Label htmlFor="p-pub">{t("site.admin.published")}</Label></div>
            <div className="flex items-center gap-2"><Switch id="p-nav" checked={p.show_in_nav} onCheckedChange={(v) => setP({ ...p, show_in_nav: v })} /><Label htmlFor="p-nav">{t("site.admin.show_in_nav")}</Label></div>
          </div>
        </CardContent>
      </Card>
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-lg font-semibold">{t("site.admin.blocks")}</h2>
        <DropdownMenu>
          <DropdownMenuTrigger asChild><Button type="button" variant="outline" size="sm"><PlusIcon />{t("site.admin.add_block")}</Button></DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {BLOCK_TYPES.map((bt) => <DropdownMenuItem key={bt} onSelect={() => { setB([...p.blocks, newBlock(bt)]); setOpen(p.blocks.length); }}>{t(`site.block.${bt}`)}</DropdownMenuItem>)}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {p.blocks.length === 0 && <p className="text-sm text-muted-foreground">{t("site.admin.no_blocks")}</p>}
      {p.blocks.map((b, i) => (
        <Card key={b.id ?? i}>
          <CardHeader className="flex flex-row flex-wrap items-center gap-2">
            <Badge variant="secondary">{t(`site.block.${b.type}`)}</Badge>
            <span className="min-w-0 flex-1 truncate text-sm text-muted-foreground">{String(b.props.title ?? "")}</span>
            <div className="flex gap-1">
              <Button type="button" variant="outline" size="icon" className="size-11 md:size-9" aria-label={t("common.edit")} onClick={() => setOpen(open === i ? null : i)}><PencilSimpleIcon /></Button>
              <Button type="button" variant="outline" size="icon" className="size-11 md:size-9" disabled={i === 0} aria-label={t("site.admin.up")} onClick={() => setB(moveBlock(p.blocks, i, i - 1))}><ArrowUpIcon /></Button>
              <Button type="button" variant="outline" size="icon" className="size-11 md:size-9" disabled={i === p.blocks.length - 1} aria-label={t("site.admin.down")} onClick={() => setB(moveBlock(p.blocks, i, i + 1))}><ArrowDownIcon /></Button>
              <Button type="button" variant="outline" size="icon" className="size-11 md:size-9" aria-label={t("common.delete")} onClick={() => setB(p.blocks.filter((_, j) => j !== i))}><TrashIcon /></Button>
            </div>
          </CardHeader>
          {open === i && <CardContent><BlockEditor block={b} onChange={(nb) => setB(p.blocks.map((x, j) => (j === i ? nb : x)))} /></CardContent>}
        </Card>
      ))}
      <div className="flex justify-end gap-2">
        <Button type="button" variant="outline" onClick={onDone}>{t("common.cancel")}</Button>
        <Button type="submit" disabled={save.isPending}><FloppyDiskIcon />{t("common.save")}</Button>
      </div>
    </form>
  );
}

/** "Product landing page" wizard: name + products → hero, features, plans, comparison, FAQ, CTA (draft). */
function LandingTemplateDialog({ onCreated }: { onCreated: (p: SitePage) => void }) {
  const { t } = useI18n();
  const err = useErr();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [ids, setIds] = useState<string[]>([]);
  const create = useMutation({
    mutationFn: () => api<SitePage>("/admin/site/pages", { method: "POST", body: { ...productLanding(name, ids), published: false, show_in_nav: false, sort_order: 0 } }),
    onSuccess: (pg) => { setOpen(false); setName(""); setIds([]); toast.success(t("common.saved")); onCreated(normalizePages([pg])[0] ?? pg); },
    onError: err,
  });
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button variant="outline" data-testid="landing-template"><RocketLaunchIcon />{t("site.admin.landing_template")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto">
        <DialogHeader><DialogTitle>{t("site.admin.landing_template")}</DialogTitle></DialogHeader>
        <p className="text-sm text-muted-foreground">{t("site.admin.landing_hint")}</p>
        <div className="flex flex-col gap-2"><Label htmlFor="lt-name">{t("site.admin.page_title")}</Label><Input id="lt-name" value={name} onChange={(e) => setName(e.target.value)} placeholder="VPS Hosting" /></div>
        <ProductPicker value={ids} onChange={setIds} />
        <DialogFooter><Button disabled={!name.trim() || create.isPending} onClick={() => create.mutate()}>{t("site.admin.create_draft")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function PagesTab() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const err = useErr();
  const q = useQuery({ queryKey: ["admin", "site", "pages"], queryFn: async () => normalizePages(await api<unknown>("/admin/site/pages")) });
  const [editing, setEditing] = useState<SitePage | null | "new">(null);
  const del = useMutation({
    mutationFn: (id: string) => api(`/admin/site/pages/${id}`, { method: "DELETE" }),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ["admin", "site", "pages"] }); toast.success(t("site.admin.deleted")); },
    onError: err,
  });
  if (editing) return <PageEditor page={editing === "new" ? null : editing} onDone={() => setEditing(null)} />;
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap justify-end gap-2">
        <LandingTemplateDialog onCreated={(pg) => { void qc.invalidateQueries({ queryKey: ["admin", "site", "pages"] }); setEditing(pg); }} />
        <Button onClick={() => setEditing("new")}><PlusIcon />{t("site.admin.new_page")}</Button>
      </div>
      {(q.data ?? []).map((p) => (
        <Card key={p.id}>
          <CardContent className="flex flex-col gap-3 sm:flex-row sm:items-center">
            <div className="min-w-0 flex-1">
              <p className="truncate font-medium">{p.title}</p>
              <p className="truncate text-sm text-muted-foreground">/page/{p.slug} · {t("site.admin.blocks_count", { n: String(p.blocks.length) })}</p>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant={p.published ? "default" : "outline"}>{p.published ? t("site.admin.published") : t("site.admin.draft")}</Badge>
              {p.show_in_nav && <Badge variant="secondary">{t("site.admin.in_nav")}</Badge>}
              <Button asChild variant="outline" size="icon" className="size-11 md:size-9" aria-label={t("site.admin.view")}><a href={`/page/${p.slug}`} target="_blank" rel="noreferrer"><ArrowSquareOutIcon /></a></Button>
              <Button variant="outline" size="icon" className="size-11 md:size-9" aria-label={t("common.edit")} onClick={() => setEditing(p)}><PencilSimpleIcon /></Button>
              <Button variant="outline" size="icon" className="size-11 md:size-9" aria-label={t("common.delete")} onClick={() => { if (confirm(t("site.admin.confirm_delete"))) del.mutate(p.id); }}><TrashIcon /></Button>
            </div>
          </CardContent>
        </Card>
      ))}
      {q.data?.length === 0 && <p className="text-sm text-muted-foreground">{t("site.admin.no_pages")}</p>}
    </div>
  );
}

export function AdminWebsitePage() {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("nav.website")} description={t("site.admin.subtitle")} />
      <Tabs defaultValue="settings">
        <TabsList><TabsTrigger value="settings">{t("site.admin.tab_settings")}</TabsTrigger><TabsTrigger value="pages">{t("site.admin.tab_pages")}</TabsTrigger></TabsList>
        <TabsContent value="settings" className="pt-2"><SettingsTab /></TabsContent>
        <TabsContent value="pages" className="pt-2"><PagesTab /></TabsContent>
      </Tabs>
    </div>
  );
}

/** Public site look; store/marketplace need their module, `auto` follows the modules. */
function PublicModeField({ value, onChange }: { value: PublicModeSetting; onChange: (v: PublicModeSetting) => void }) {
  const { t } = useI18n();
  const { enabled } = useModules();
  const need: Partial<Record<PublicModeSetting, string>> = { store: "gaming_storefront", marketplace: "creator_marketplace" };
  const active = resolveMode(value, enabled);
  return (
    <div className="flex flex-col gap-2 sm:col-span-2">
      <Label htmlFor="s-public-mode">{t("site.admin.public_mode")}</Label>
      <Select value={value} onValueChange={(v) => onChange(v as PublicModeSetting)}>
        <SelectTrigger id="s-public-mode" className="w-full sm:w-80" aria-describedby="s-public-mode-hint"><SelectValue /></SelectTrigger>
        <SelectContent>
          {PUBLIC_MODES.map((m) => (
            <SelectItem key={m} value={m} disabled={!!need[m] && !enabled.has(need[m]!) && m !== value}>{t(`site.admin.public_mode_${m}`)}</SelectItem>
          ))}
        </SelectContent>
      </Select>
      <p id="s-public-mode-hint" className="text-xs text-muted-foreground">{t("site.admin.public_mode_hint", { mode: t(`site.admin.public_mode_${active}`) })}</p>
    </div>
  );
}
