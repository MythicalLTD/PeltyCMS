/**
 * Native renderer for declarative plugin UI (no plugin JS; addon iframes
 * only through the sandboxed `frame` node, see AddonFrame.tsx).
 * Every node maps to a shadcn component and therefore follows the active
 * theme's CSS variables. Unknown nodes render nothing (the server rejects
 * them at manifest load anyway).
 */
import { createContext, useContext, useState, type ReactNode } from "react";
import { useNavigate, Link } from "react-router";
import { useQueries, useQueryClient } from "@tanstack/react-query";
import * as P from "@phosphor-icons/react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { Badge } from "@/components/ui/badge";
import { Button, buttonVariants } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { Card, CardAction, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Breadcrumb, BreadcrumbItem, BreadcrumbLink, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator } from "@/components/ui/breadcrumb";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetTrigger, SheetDescription } from "@/components/ui/sheet";
import { AddonFrame } from "@/components/plugin-ui/AddonFrame";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import { Bar, BarChart, CartesianGrid, Line, LineChart, XAxis } from "recharts";
import { api, ApiError } from "@/lib/api";
import { formatMoney, formatDate } from "@/lib/billing/billing";
import { ConfirmAction } from "@/components/ConfirmAction";
import { useI18n } from "@/lib/i18n";
import { apiRel, display, fillPath, resolve, text, truthy, type DataSource, type Scope, type UiNode } from "@/lib/addons/plugin-ui";

/** Curated icon set (keeps the bundle small). Unknown names render nothing. */
import { ICONS } from "@/components/plugin-ui/icons";
export { ICONS };

interface Ctx { plugin: string; refresh: () => void; bridge: string[]; sameOrigin?: boolean }
const RenderCtx = createContext<Ctx>({ plugin: "", refresh: () => {}, bridge: [] });

type Props = Record<string, unknown>;
const str = (v: unknown) => (typeof v === "string" ? v : undefined);
const num = (v: unknown, d: number) => (Number.isFinite(Number(v)) && v !== "" && v !== null && v !== undefined ? Number(v) : d);

/** Load a node tree's data sources; returns scope + refresh. */
export function usePluginData(plugin: string, data: Record<string, DataSource>, base: Scope) {
  const { tCode } = useI18n();
  const qc = useQueryClient();
  const entries = Object.entries(data ?? {});
  const results = useQueries({
    queries: entries.map(([name, src]) => {
      const p = fillPath(src.get, base, tCode);
      const rel = p ? apiRel(p) : null;
      return {
        queryKey: ["plugin-ui", plugin, name, rel],
        queryFn: () => api<unknown>(rel!),
        enabled: !!rel,
        refetchInterval: src.refresh_secs ? src.refresh_secs * 1000 : (false as const),
      };
    }),
  });
  const d: Record<string, unknown> = {};
  const errors: Record<string, string> = {};
  entries.forEach(([name], i) => {
    d[name] = results[i]?.data;
    const e = results[i]?.error;
    if (e) errors[name] = e instanceof ApiError ? e.code : "error.internal";
  });
  return {
    scope: { ...base, data: d, loading: results.some((r) => r.isLoading), errors } as Scope,
    refresh: () => void qc.invalidateQueries({ queryKey: ["plugin-ui", plugin] }),
  };
}

/** Root: loads data and renders a node tree. */
export function PluginView({ plugin, data, body, base, frame, bridge = [], sameOrigin = false }: { plugin: string; data: Record<string, DataSource>; body: UiNode | null; base: Scope; frame?: { src: string; height?: number | null } | null; bridge?: string[]; sameOrigin?: boolean }) {
  const { scope, refresh } = usePluginData(plugin, data, base);
  if (frame) return <AddonFrame plugin={plugin} frame={frame} bridge={bridge} sameOrigin={sameOrigin} ctx={(base.ctx as Record<string, unknown>) ?? {}} />;
  if (!body) return null;
  return (
    <RenderCtx.Provider value={{ plugin, refresh, bridge, sameOrigin }}>
      <Node node={body} scope={scope} />
    </RenderCtx.Provider>
  );
}

function Children({ nodes, scope }: { nodes: unknown; scope: Scope }) {
  if (!Array.isArray(nodes)) return null;
  return <>{nodes.map((n, i) => <Node key={i} node={n as UiNode} scope={scope} />)}</>;
}

const GAP: Record<string, string> = { none: "gap-0", xs: "gap-1", sm: "gap-2", md: "gap-4", lg: "gap-6" };
const COLS: Record<number, string> = { 1: "sm:grid-cols-1", 2: "sm:grid-cols-2", 3: "sm:grid-cols-2 lg:grid-cols-3", 4: "sm:grid-cols-2 lg:grid-cols-4", 5: "sm:grid-cols-3 lg:grid-cols-5", 6: "sm:grid-cols-3 lg:grid-cols-6" };
const BADGE = ["default", "secondary", "destructive", "outline"] as const;
const BTN = ["default", "secondary", "destructive", "outline", "ghost", "link"] as const;
const pick = <T extends string>(v: unknown, opts: readonly T[], d: T): T => (opts.includes(v as T) ? (v as T) : d);

export function Node({ node, scope }: { node: UiNode; scope: Scope }): ReactNode {
  const { tCode, lang } = useI18n();
  if (!node || typeof node !== "object") return null;
  const n = node as Props;
  const r = (k: string) => resolve(n[k], scope, tCode);
  const tx = (k: string) => (n[k] === undefined ? "" : text(n[k], scope, tCode));
  const kids = <Children nodes={n.children} scope={scope} />;
  if (n.visible !== undefined && !truthy(r("visible"))) return null;
  switch (node.type) {
    case "stack": return <div className={`flex flex-col ${GAP[str(n.gap) ?? "md"] ?? "gap-4"}`}>{kids}</div>;
    case "row": return <div className={`flex flex-wrap items-center ${GAP[str(n.gap) ?? "sm"] ?? "gap-2"} ${n.justify === "end" ? "justify-end" : n.justify === "between" ? "justify-between" : ""}`}>{kids}</div>;
    case "grid": return <div className={`grid ${GAP[str(n.gap) ?? "md"] ?? "gap-4"} ${COLS[num(n.columns_count, 3)] ?? COLS[3]}`}>{kids}</div>;
    case "section": return <section className="flex flex-col gap-3">{n.title ? <h3 className="text-sm font-medium text-muted-foreground">{tx("title")}</h3> : null}{kids}</section>;
    case "card": return (
      <Card>
        {(n.title || n.description) ? <CardHeader>{n.title ? <CardTitle>{tx("title")}</CardTitle> : null}{n.description ? <CardDescription>{tx("description")}</CardDescription> : null}{Array.isArray(n.actions) ? <CardAction><div className="flex gap-1"><Children nodes={n.actions} scope={scope} /></div></CardAction> : null}</CardHeader> : null}
        <CardContent className="flex flex-col gap-3">{kids}</CardContent>
        {Array.isArray(n.footer) ? <CardFooter className="gap-2"><Children nodes={n.footer} scope={scope} /></CardFooter> : null}
      </Card>
    );
    case "separator": return <Separator />;
    case "heading": {
      const lvl = num(n.level, 1);
      const cls = lvl <= 1 ? "text-xl font-semibold" : lvl === 2 ? "text-lg font-semibold" : "text-base font-medium";
      return <div className="flex flex-col gap-1"><h2 className={cls}>{tx("text")}</h2>{n.description ? <p className="text-sm text-muted-foreground">{tx("description")}</p> : null}</div>;
    }
    case "text": {
      const tone = n.tone === "muted" ? "text-muted-foreground" : n.tone === "destructive" ? "text-destructive" : "";
      const size = n.size === "sm" ? "text-sm" : n.size === "xs" ? "text-xs" : "";
      return <p className={`${tone} ${size} ${n.mono ? "font-mono" : ""} ${n.weight === "medium" ? "font-medium" : ""}`.trim() || undefined}>{tx("text")}</p>;
    }
    case "badge": return <Badge variant={pick(r("variant"), BADGE, "secondary")}>{tx("text")}</Badge>;
    case "alert": return (
      <Alert variant={n.variant === "destructive" ? "destructive" : "default"}>
        {n.title ? <AlertTitle>{tx("title")}</AlertTitle> : null}
        <AlertDescription>{tx("text")}</AlertDescription>
      </Alert>
    );
    case "stat": return (
      <Card><CardContent className="flex flex-col gap-1">
        <p className="text-xs text-muted-foreground">{tx("label")}</p>
        <p className="font-mono text-2xl font-semibold tabular-nums">{scope.loading ? <Skeleton className="h-8 w-24" /> : tx("value") || "-"}</p>
        {n.hint ? <p className="text-xs text-muted-foreground">{tx("hint")}</p> : null}
      </CardContent></Card>
    );
    case "kv": {
      const items = Array.isArray(n.items) ? (n.items as Props[]) : [];
      return (
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
          {items.map((it, i) => <KvRow key={i} label={text(it.label, scope, tCode)} value={text(it.value, scope, tCode)} mono={!!it.mono} />)}
        </dl>
      );
    }
    case "code": return <pre className="overflow-x-auto rounded-md border bg-muted p-3 font-mono text-xs">{display(r("text"))}</pre>;
    case "image": { const src = fillPath(str(n.src) ?? "", scope, tCode); return src ? <img src={src} alt={tx("alt")} className="max-w-full rounded-md border" loading="lazy" /> : null; }
    case "avatar": return <Avatar>{str(r("src")) && fillPath(str(n.src)!, scope, tCode) ? <AvatarImage src={fillPath(str(n.src)!, scope, tCode)!} /> : null}<AvatarFallback>{tx("fallback").slice(0, 2).toUpperCase()}</AvatarFallback></Avatar>;
    case "icon": { const I = ICONS[str(n.name) ?? ""]; return I ? <I aria-hidden className={n.tone === "muted" ? "text-muted-foreground" : undefined} /> : null; }
    case "progress": {
      const max = num(r("max"), 100) || 100;
      const v = Math.max(0, Math.min(max, num(r("value"), 0)));
      const pct = Math.round((v / max) * 100);
      return (
        <div className="flex items-center gap-2">
          <progress className="h-2 w-full overflow-hidden rounded-full accent-primary [&::-webkit-progress-bar]:rounded-full [&::-webkit-progress-bar]:bg-muted [&::-webkit-progress-value]:rounded-full [&::-webkit-progress-value]:bg-primary" value={v} max={max} />
          <span className="w-10 text-right text-xs tabular-nums text-muted-foreground">{pct}%</span>
        </div>
      );
    }
    case "skeleton": return <Skeleton className="h-6 w-full" />;
    case "empty": return <Empty><EmptyHeader><EmptyTitle>{tx("title")}</EmptyTitle>{n.description ? <EmptyDescription>{tx("description")}</EmptyDescription> : null}</EmptyHeader></Empty>;
    case "tooltip": return <Tooltip><TooltipTrigger asChild><span>{kids}</span></TooltipTrigger><TooltipContent>{tx("text")}</TooltipContent></Tooltip>;
    case "money": return <span className="tabular-nums">{formatMoney(display(r("amount")) || "0", display(r("currency")) || "EUR", lang)}</span>;
    case "date": { const v = display(r("value")); return <span>{v ? formatDate(v, lang) : "-"}</span>; }
    case "link": { const to = fillPath(str(n.href) ?? "", scope, tCode); return to ? <Link className="text-primary underline-offset-4 hover:underline" to={to}>{tx("label")}</Link> : null; }
    case "if": return truthy(r("when")) ? kids : (n.else ? <Node node={n.else as UiNode} scope={scope} /> : null);
    case "each": {
      const items = r("items");
      if (!Array.isArray(items) || items.length === 0) return n.empty ? <Node node={n.empty as UiNode} scope={scope} /> : null;
      return <>{items.map((it, i) => <Node key={i} node={n.template as UiNode} scope={{ ...scope, item: it, index: i }} />)}</>;
    }
    case "list": {
      const items = r("items");
      if (!Array.isArray(items) || items.length === 0) return n.empty ? <Node node={n.empty as UiNode} scope={scope} /> : null;
      return <ul className="flex flex-col divide-y rounded-md border">{items.map((it, i) => <li key={i} className="p-3"><Node node={n.template as UiNode} scope={{ ...scope, item: it, index: i }} /></li>)}</ul>;
    }
    case "table": return <TableNode n={n} scope={scope} />;
    case "chart": return <ChartNode n={n} scope={scope} />;
    case "tabs": {
      const tabs = (Array.isArray(n.children) ? n.children : []) as Props[];
      if (!tabs.length) return null;
      return (
        <Tabs defaultValue="0">
          <TabsList>{tabs.map((tb, i) => <TabsTrigger key={i} value={String(i)}>{text(tb.label, scope, tCode)}</TabsTrigger>)}</TabsList>
          {tabs.map((tb, i) => <TabsContent key={i} value={String(i)} className="flex flex-col gap-3 pt-2"><Children nodes={tb.children} scope={scope} /></TabsContent>)}
        </Tabs>
      );
    }
    case "tab": return kids;
    case "accordion": return <div className="flex flex-col divide-y rounded-md border">{kids}</div>;
    case "accordion_item": return <details className="group p-3"><summary className="cursor-pointer text-sm font-medium">{tx("label")}</summary><div className="flex flex-col gap-3 pt-3">{kids}</div></details>;
    case "dialog": return <DialogNode n={n} scope={scope} />;
    case "button": return <ActionButton n={n} scope={scope} />;
    case "form": return <FormNode n={n} scope={scope} />;
    case "input": case "textarea": case "select": case "switch": case "checkbox": case "radio":
      return <FieldNode n={n} type={node.type} scope={scope} />;
    case "embed": return <EmbedNode n={n} scope={scope} />;
    case "frame": return <FrameNode n={n} scope={scope} />;
    case "collapsible": return (
      <Collapsible defaultOpen={!!n.open} className="flex flex-col gap-2 rounded-md border p-3">
        <CollapsibleTrigger asChild><Button variant="ghost" size="sm" className="justify-start"><P.CaretDownIcon aria-hidden />{tx("label")}</Button></CollapsibleTrigger>
        <CollapsibleContent className="flex flex-col gap-3">{kids}</CollapsibleContent>
      </Collapsible>
    );
    case "popover": return (
      <Popover>
        <PopoverTrigger asChild><Button size="sm" variant={pick(n.variant, BTN, "outline")}>{tx("label")}</Button></PopoverTrigger>
        <PopoverContent className="flex flex-col gap-3">{kids}</PopoverContent>
      </Popover>
    );
    case "sheet": return (
      <Sheet>
        <SheetTrigger asChild><Button size="sm" variant={pick(n.variant, BTN, "outline")}>{tx("label")}</Button></SheetTrigger>
        <SheetContent side={n.side === "left" ? "left" : "right"} className="overflow-y-auto">
          <SheetHeader><SheetTitle>{text(n.title ?? n.label, scope, tCode)}</SheetTitle>{n.description ? <SheetDescription>{tx("description")}</SheetDescription> : null}</SheetHeader>
          <div className="flex flex-col gap-3 px-4 pb-4">{kids}</div>
        </SheetContent>
      </Sheet>
    );
    case "dropdown": return <DropdownNode n={n} scope={scope} />;
    case "breadcrumb": {
      const items = Array.isArray(n.items) ? (n.items as Props[]) : [];
      return (
        <Breadcrumb><BreadcrumbList>
          {items.map((it, i) => {
            const to = it.href ? fillPath(str(it.href) ?? "", scope, tCode) : null;
            const label = text(it.label, scope, tCode);
            return (
              <span key={i} className="contents">
                {i > 0 ? <BreadcrumbSeparator /> : null}
                <BreadcrumbItem>{to && i < items.length - 1 ? <BreadcrumbLink asChild><Link to={to}>{label}</Link></BreadcrumbLink> : <BreadcrumbPage>{label}</BreadcrumbPage>}</BreadcrumbItem>
              </span>
            );
          })}
        </BreadcrumbList></Breadcrumb>
      );
    }
    case "pagination": return <PaginationNode n={n} scope={scope} />;
    case "scroll_area": return <div className="overflow-auto rounded-md border p-3" style={{ maxHeight: num(n.max_height, 320) }}>{kids}</div>;
    case "label": return <Label>{tx("text")}</Label>;
    case "kbd": return <kbd className="rounded border bg-muted px-1.5 py-0.5 font-mono text-xs">{tx("text")}</kbd>;
    case "blockquote": return <blockquote className="border-l-2 pl-4 text-muted-foreground italic">{tx("text")}{kids}</blockquote>;
    case "list_item": return <li>{tx("text")}{kids}</li>;
    default: return null;
  }
}

function KvRow({ label, value, mono }: { label: string; value: string; mono: boolean }) {
  return <><dt className="text-muted-foreground">{label}</dt><dd className={mono ? "break-all font-mono text-xs" : "break-words"}>{value || "-"}</dd></>;
}

function TableNode({ n, scope }: { n: Props; scope: Scope }) {
  const { tCode } = useI18n();
  const rows = resolve(n.rows, scope, tCode);
  const cols = (Array.isArray(n.columns) ? n.columns : []) as Props[];
  if (scope.loading && !Array.isArray(rows)) return <Skeleton className="h-24 w-full" />;
  if (!Array.isArray(rows) || rows.length === 0) return n.empty ? <Node node={n.empty as UiNode} scope={scope} /> : null;
  return (
    <div className="overflow-x-auto">
      <Table>
        <TableHeader><TableRow>{cols.map((c, i) => <TableHead key={i}>{c.header ? text(c.header, scope, tCode) : ""}</TableHead>)}</TableRow></TableHeader>
        <TableBody>
          {rows.map((row, ri) => {
            const s = { ...scope, row, index: ri };
            return (
              <TableRow key={ri}>
                {cols.map((c, ci) => <TableCell key={ci}>{c.cell ? <Node node={c.cell as UiNode} scope={s} /> : text(c.value, s, tCode)}</TableCell>)}
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </div>
  );
}

function ChartNode({ n, scope }: { n: Props; scope: Scope }) {
  const { tCode } = useI18n();
  const rows = resolve(n.rows, scope, tCode);
  const x = str(n.x) ?? "x";
  const series = (Array.isArray(n.series) ? n.series : []) as Props[];
  if (!Array.isArray(rows) || !series.length) return null;
  const config: ChartConfig = Object.fromEntries(series.map((s, i) => [str(s.key) ?? `s${i}`, { label: text(s.label, scope, tCode), color: `var(--chart-${(i % 5) + 1})` }]));
  const data = rows as Record<string, unknown>[];
  return (
    <ChartContainer config={config} className="h-56 w-full">
      {n.kind === "line" ? (
        <LineChart data={data}><CartesianGrid vertical={false} /><XAxis dataKey={x} tickLine={false} axisLine={false} /><ChartTooltip content={<ChartTooltipContent />} />
          {series.map((s, i) => <Line key={i} dataKey={str(s.key) ?? ""} stroke={`var(--color-${str(s.key)})`} dot={false} />)}</LineChart>
      ) : (
        <BarChart data={data}><CartesianGrid vertical={false} /><XAxis dataKey={x} tickLine={false} axisLine={false} /><ChartTooltip content={<ChartTooltipContent />} />
          {series.map((s, i) => <Bar key={i} dataKey={str(s.key) ?? ""} fill={`var(--color-${str(s.key)})`} radius={4} />)}</BarChart>
      )}
    </ChartContainer>
  );
}

/** Runs a validated action with the viewer's own session. */
function useRunAction() {
  const { tCode, t } = useI18n();
  const nav = useNavigate();
  const { refresh } = useContext(RenderCtx);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const run = async (action: Props, scope: Scope, extraBody?: Record<string, unknown>): Promise<void> => {
    const type = action.type;
    if (type === "refresh") { refresh(); return; }
    if (type === "navigate") { const to = fillPath(str(action.to) ?? "", scope, tCode); if (to) void nav(to); return; }
    if (type !== "request") return;
    const p = fillPath(str(action.path) ?? "", scope, tCode);
    const rel = p ? apiRel(p) : null;
    if (!rel) { setMsg({ ok: false, text: t("error.internal") }); return; }
    setBusy(true); setMsg(null);
    try {
      const base = action.body ? (resolve(action.body, scope, tCode) as Record<string, unknown>) : {};
      const body = extraBody ? { ...base, ...extraBody } : action.body ? base : undefined;
      const res = await api<unknown>(rel, { method: str(action.method) ?? "POST", body });
      if (action.success) setMsg({ ok: true, text: text(action.success, scope, tCode) });
      if (action.then) await run(action.then as Props, { ...scope, result: res });
      else refresh();
    } catch (e) {
      setMsg({ ok: false, text: e instanceof ApiError ? tCode(e.code) : t("error.internal") });
    } finally { setBusy(false); }
  };
  return { run, busy, msg };
}

function Msg({ m }: { m: { ok: boolean; text: string } | null }) {
  if (!m) return null;
  return <p role={m.ok ? "status" : "alert"} className={m.ok ? "text-sm text-muted-foreground" : "text-sm text-destructive"}>{m.text}</p>;
}

function ActionButton({ n, scope }: { n: Props; scope: Scope }) {
  const { tCode } = useI18n();
  const { run, busy, msg } = useRunAction();
  const I = ICONS[str(n.icon) ?? ""];
  const disabled = n.disabled !== undefined && truthy(resolve(n.disabled, scope, tCode));
  return (
    <span className="inline-flex flex-col gap-1">
      {(() => {
        const go = () => { if (n.action) void run(n.action as Props, scope); };
        const btn = (
          <Button size={n.size === "sm" ? "sm" : "default"} variant={pick(n.variant, BTN, "default")} disabled={busy || disabled} onClick={n.confirm ? undefined : go}>
            {I ? <I aria-hidden /> : null}{text(n.label, scope, tCode)}
          </Button>
        );
        return n.confirm ? <ConfirmAction description={text(n.confirm, scope, tCode)} destructive={n.variant === "destructive"} onConfirm={go}>{btn}</ConfirmAction> : btn;
      })()}
      <Msg m={msg} />
    </span>
  );
}

function DialogNode({ n, scope }: { n: Props; scope: Scope }) {
  const { tCode } = useI18n();
  return (
    <Dialog>
      <DialogTrigger asChild><Button size="sm" variant={pick(n.variant, BTN, "outline")}>{text(n.label, scope, tCode)}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto">
        <DialogHeader><DialogTitle>{text(n.title ?? n.label, scope, tCode)}</DialogTitle></DialogHeader>
        <div className="flex flex-col gap-3"><Children nodes={n.children} scope={scope} /></div>
        {Array.isArray(n.footer) ? <DialogFooter><Children nodes={n.footer} scope={scope} /></DialogFooter> : null}
      </DialogContent>
    </Dialog>
  );
}

/** Form state shared with its field nodes. */
const FormCtx = createContext<{ values: Record<string, unknown>; set: (k: string, v: unknown) => void } | null>(null);

function initialValues(nodes: unknown, scope: Scope, t: (k: string) => string, out: Record<string, unknown> = {}) {
  if (!Array.isArray(nodes)) return out;
  for (const c of nodes as Props[]) {
    if (typeof c.name === "string") out[c.name] = c.value !== undefined ? resolve(c.value, scope, t) : c.type === "switch" || c.type === "checkbox" ? false : "";
    initialValues(c.children, scope, t, out);
  }
  return out;
}

function FormNode({ n, scope }: { n: Props; scope: Scope }) {
  const { tCode } = useI18n();
  const [values, setValues] = useState<Record<string, unknown>>(() => initialValues(n.children, scope, tCode));
  const { run, busy, msg } = useRunAction();
  return (
    <FormCtx.Provider value={{ values, set: (k, v) => setValues((x) => ({ ...x, [k]: v })) }}>
      <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); if (n.submit) void run(n.submit as Props, { ...scope, form: values }, values); }}>
        <Children nodes={n.children} scope={{ ...scope, form: values }} />
        <Msg m={msg} />
        <div><Button type="submit" disabled={busy}>{n.submit_label ? text(n.submit_label, scope, tCode) : tCode("common.save")}</Button></div>
      </form>
    </FormCtx.Provider>
  );
}

function FieldNode({ n, type, scope }: { n: Props; type: string; scope: Scope }) {
  const { tCode } = useI18n();
  const f = useContext(FormCtx);
  const name = str(n.name) ?? "";
  const id = `pf-${name}`;
  const value = f?.values[name];
  const set = (v: unknown) => f?.set(name, v);
  const label = n.label ? text(n.label, scope, tCode) : "";
  const hint = n.hint ? <p className="text-sm text-muted-foreground">{text(n.hint, scope, tCode)}</p> : null;
  const opts = (() => { const o = resolve(n.options, scope, tCode); return Array.isArray(o) ? (o as Props[]).map((x) => ({ value: display(x.value), label: text(x.label ?? x.value, scope, tCode) })) : []; })();
  if (type === "switch" || type === "checkbox") {
    return (
      <div className="flex items-center justify-between gap-3 rounded-md border p-3">
        <div><Label htmlFor={id}>{label}</Label>{hint}</div>
        {type === "switch" ? <Switch id={id} checked={!!value} onCheckedChange={set} /> : <Checkbox id={id} checked={!!value} onCheckedChange={(v) => set(v === true)} />}
      </div>
    );
  }
  return (
    <div className="grid gap-2">
      <Label htmlFor={id}>{label}{n.required ? " *" : ""}</Label>
      {type === "textarea" ? <Textarea id={id} required={!!n.required} rows={num(n.rows, 4)} value={display(value)} onChange={(e) => set(e.target.value)} />
        : type === "select" ? (
          <Select value={display(value)} onValueChange={set}>
            <SelectTrigger id={id} className="w-full"><SelectValue placeholder={n.placeholder ? text(n.placeholder, scope, tCode) : undefined} /></SelectTrigger>
            <SelectContent>{opts.map((o) => <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>)}</SelectContent>
          </Select>
        ) : type === "radio" ? (
          <div role="radiogroup" className="flex flex-wrap gap-2">
            {opts.map((o) => (
              <label key={o.value} className={cn(buttonVariants({ size: "sm", variant: display(value) === o.value ? "default" : "outline" }), "cursor-pointer")}>
                <input type="radio" name={id} className="sr-only" checked={display(value) === o.value} onChange={() => set(o.value)} />
                {o.label}
              </label>
            ))}
          </div>
        ) : (
          <Input id={id} required={!!n.required} type={n.input_type === "number" ? "number" : n.input_type === "email" ? "email" : n.input_type === "password" ? "password" : "text"}
            placeholder={n.placeholder ? text(n.placeholder, scope, tCode) : undefined} value={display(value)}
            onChange={(e) => set(n.input_type === "number" ? (e.target.value === "" ? "" : Number(e.target.value)) : e.target.value)} />
        )}
      {hint}
    </div>
  );
}

function FrameNode({ n, scope }: { n: Props; scope: Scope }) {
  const { plugin, bridge, sameOrigin } = useContext(RenderCtx);
  const src = str(n.src);
  if (!src || !src.startsWith("assets/")) return null;
  return <AddonFrame plugin={plugin} frame={{ src: `/api/v1/addons/${plugin}/${src}`, height: num(n.height, 320) }} bridge={bridge} sameOrigin={sameOrigin} ctx={(scope.ctx as Record<string, unknown>) ?? {}} />;
}

function DropdownNode({ n, scope }: { n: Props; scope: Scope }) {
  const { tCode } = useI18n();
  const { run } = useRunAction();
  const items = Array.isArray(n.items) ? (n.items as Props[]) : [];
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild><Button size="sm" variant={pick(n.variant, BTN, "outline")}>{text(n.label, scope, tCode)}<P.CaretDownIcon aria-hidden /></Button></DropdownMenuTrigger>
      <DropdownMenuContent>
        {items.map((it, i) => {
          const I = ICONS[str(it.icon) ?? ""];
          return <DropdownMenuItem key={i} variant={it.variant === "destructive" ? "destructive" : "default"} onSelect={() => { if (it.action) void run(it.action as Props, scope); }}>{I ? <I aria-hidden /> : null}{text(it.label, scope, tCode)}</DropdownMenuItem>;
        })}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** Prev/next over `params.page` (navigates with `?page=`). */
function PaginationNode({ n, scope }: { n: Props; scope: Scope }) {
  const { t, tCode } = useI18n();
  const nav = useNavigate();
  const page = Math.max(1, num(resolve(n.page, scope, tCode), 1));
  const pages = Math.max(1, num(resolve(n.pages, scope, tCode), 1));
  const go = (p: number) => { const u = new URL(window.location.href); u.searchParams.set("page", String(p)); void nav(`${u.pathname}${u.search}`); };
  return (
    <div className="flex items-center justify-end gap-2 text-sm">
      <Button size="sm" variant="outline" disabled={page <= 1} onClick={() => go(page - 1)} aria-label={t("common.previous")}><P.CaretLeftIcon aria-hidden /></Button>
      <span className="tabular-nums text-muted-foreground">{page} / {pages}</span>
      <Button size="sm" variant="outline" disabled={page >= pages} onClick={() => go(page + 1)} aria-label={t("common.next")}><P.CaretRightIcon aria-hidden /></Button>
    </div>
  );
}

/** Last resort: sandboxed https iframe (needs the `ui.embed` capability). */
function EmbedNode({ n, scope }: { n: Props; scope: Scope }) {
  const { tCode } = useI18n();
  const src = str(n.src);
  if (!src || !src.startsWith("https://")) return null;
  return <iframe title={text(n.title ?? "", scope, tCode) || "plugin"} src={src} sandbox="allow-scripts allow-forms" referrerPolicy="no-referrer" className="h-96 w-full rounded-md border" />;
}

