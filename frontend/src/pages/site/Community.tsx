import { useEffect, useState } from "react";
import { useLocation } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { ArrowSquareOutIcon, CopyIcon, CrownIcon, TrophyIcon, UsersThreeIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { EmptyState } from "@/components/EmptyState";
import { LoadError } from "@/components/LoadError";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { api } from "@/lib/api";
import { formatMoney } from "@/lib/billing/billing";
import { fillPercent, voteLink, type Community, type ServerStatus } from "@/lib/community";
import { useI18n } from "@/lib/i18n";

/** Public community page: live servers, vote links, leaderboards, goal. */
export function CommunityPage() {
  const { t, lang } = useI18n();
  const q = useQuery({ queryKey: ["public", "community"], queryFn: () => api<Community>("/public/community"), refetchInterval: 60_000 });
  const [name, setName] = useState("");
  const { hash } = useLocation();
  const ready = !!q.data;
  // `/community#vote` and `#leaderboards` (store nav) jump once the data is in.
  useEffect(() => {
    if (ready && hash) document.getElementById(decodeURIComponent(hash.slice(1)))?.scrollIntoView({ block: "start" });
  }, [ready, hash]);
  if (q.isError && !q.data) return <LoadError retry={() => void q.refetch()} />;
  if (!q.data) return <Skeleton className="h-64 w-full" />;
  const d = q.data;
  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-semibold">{t("community.title")}</h1>
        <p className="text-muted-foreground">{t("community.subtitle")}</p>
      </header>
      {d.goal && (
        <Card>
          <CardHeader><CardTitle>{d.goal.label || t("community.goal")}</CardTitle>
            <CardDescription className="tabular-nums">{t("community.goal_progress", { raised: formatMoney(d.goal.raised, d.goal.currency, lang), amount: formatMoney(d.goal.amount, d.goal.currency, lang) })}</CardDescription>
          </CardHeader>
          <CardContent>
            <progress className="h-3 w-full overflow-hidden rounded-full accent-primary [&::-webkit-progress-bar]:bg-muted [&::-webkit-progress-value]:bg-primary [&::-moz-progress-bar]:bg-primary" aria-label={t("community.goal")} value={d.goal.percent} max={100} />
          </CardContent>
        </Card>
      )}
      <section aria-labelledby="cm-servers" className="flex flex-col gap-3">
        <h2 id="cm-servers" className="text-lg font-semibold">{t("community.servers")}</h2>
        {d.servers.length === 0 ? <EmptyState icon={UsersThreeIcon} title={t("community.no_servers")} /> : (
          <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">{d.servers.map((s) => <ServerCard key={s.server_id} s={s} />)}</div>
        )}
      </section>
      <div id="leaderboards" className="grid scroll-mt-20 gap-4 lg:grid-cols-3">
        {d.sites.length > 0 && (
          <Card id="vote" className="scroll-mt-20">
            <CardHeader><CardTitle>{t("community.vote")}</CardTitle><CardDescription>{t("community.vote_hint")}</CardDescription></CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="grid gap-1"><Label htmlFor="cm-name">{t("community.your_name")}</Label><Input id="cm-name" maxLength={64} value={name} onChange={(e) => setName(e.target.value)} /></div>
              {d.sites.map((v) => (
                <Button key={v.id} asChild variant="outline" className="justify-between">
                  <a href={voteLink(v.url, name)} target="_blank" rel="noopener noreferrer nofollow">{v.name}<ArrowSquareOutIcon aria-hidden /></a>
                </Button>
              ))}
            </CardContent>
          </Card>
        )}
        <Leaderboard title={t("community.top_voters")} icon={<TrophyIcon aria-hidden />} empty={t("community.no_votes")}
          rows={d.top_voters.map((v) => ({ name: v.username, value: t("community.votes", { n: v.votes }) }))} />
        {d.top_donors && <Leaderboard title={t("community.top_donors")} icon={<CrownIcon aria-hidden />} empty={t("community.no_donors")}
          rows={d.top_donors.map((x) => ({ name: x.name, value: x.total && x.currency ? formatMoney(x.total, x.currency, lang) : "" }))} />}
      </div>
    </div>
  );
}

function ServerCard({ s }: { s: ServerStatus }) {
  const { t } = useI18n();
  const copy = () => { void navigator.clipboard?.writeText(s.address).then(() => toast.success(t("community.copied"))); };
  return (
    <Card data-server={s.slug}>
      <CardHeader>
        <CardTitle className="flex items-center justify-between gap-2 text-base">
          <span>{s.name}</span>
          <Badge variant={s.online ? "secondary" : "destructive"}>{s.online ? t("community.online") : t("community.offline")}</Badge>
        </CardTitle>
        {s.motd && <CardDescription className="line-clamp-2">{s.motd}</CardDescription>}
      </CardHeader>
      <CardContent className="flex flex-col gap-2 text-sm">
        {s.online && (
          <>
            <div className="flex justify-between tabular-nums"><span>{t("community.players", { n: s.players, max: s.max_players })}</span>{s.version && <span className="text-muted-foreground">{s.version}</span>}</div>
            <div className="h-2 overflow-hidden rounded bg-muted" aria-hidden><div className="h-full bg-primary" style={{ width: `${fillPercent(s.players, s.max_players)}%` }} /></div>
            {s.sample.length > 0 && <p className="text-xs text-muted-foreground">{s.sample.join(", ")}</p>}
          </>
        )}
        {s.address && <Button variant="ghost" size="sm" className="justify-start font-mono" onClick={copy}><CopyIcon aria-hidden />{s.address}</Button>}
      </CardContent>
    </Card>
  );
}

function Leaderboard({ title, icon, rows, empty }: { title: string; icon: React.ReactNode; rows: { name: string; value: string }[]; empty: string }) {
  return (
    <Card>
      <CardHeader><CardTitle className="flex items-center gap-2">{icon}{title}</CardTitle></CardHeader>
      <CardContent>
        {rows.length === 0 ? <p className="text-sm text-muted-foreground">{empty}</p> : (
          <ol className="flex flex-col gap-1 text-sm">
            {rows.map((r, i) => <li key={`${r.name}-${i}`} className="flex justify-between gap-2"><span><span className="mr-2 tabular-nums text-muted-foreground">{i + 1}.</span>{r.name}</span><span className="tabular-nums text-muted-foreground">{r.value}</span></li>)}
          </ol>
        )}
      </CardContent>
    </Card>
  );
}
