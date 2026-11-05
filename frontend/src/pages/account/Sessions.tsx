import { PageHeader } from "@/components/PageHeader";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Card, CardHeader, CardTitle } from "@/components/ui/card";
import { ListCard } from "@/components/ListPage";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ConsentCard } from "@/components/account/ConsentCard";
import { PrivacyCard } from "@/components/account/PrivacyCard";
import { SecurityQuestionCard } from "@/components/account/SecurityQuestions";
import { TwoFactorCard } from "@/components/account/TwoFactorCard";
import { PasskeysCard } from "@/components/account/PasskeysCard";
import { PasswordChangeCard } from "@/components/account/PasswordChangeCard";
import { LinkedAccountsCard } from "@/components/account/LinkedAccountsCard";
import { ProfilePictureCard } from "@/components/account/ProfilePictureCard";
import { api } from "@/lib/api";
import { formatDateTime, parseUserAgent } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

interface Session { id: string; ip: string | null; user_agent: string | null; last_seen_at: string; current: boolean }
interface SessionGroup extends Session { ids: string[] }

/** Collapse sessions from the same browser + IP into one row (newest first);
 *  the current session always stays in its own row. */
export function groupSessions(items: Session[]): SessionGroup[] {
  const out = new Map<string, SessionGroup>();
  for (const x of [...items].sort((a, b) => b.last_seen_at.localeCompare(a.last_seen_at))) {
    const k = x.current ? `cur:${x.id}` : `${x.user_agent ?? ""}|${x.ip ?? ""}`;
    const g = out.get(k);
    if (g) g.ids.push(x.id);
    else out.set(k, { ...x, ids: [x.id] });
  }
  return [...out.values()].sort((a, b) => Number(b.current) - Number(a.current));
}

export function SessionsPage() {
  const { t, lang } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["sessions"], queryFn: () => api<{ items: Session[] }>("/auth/sessions") });
  const revoke = useMutation({
    mutationFn: (ids: string[]) => Promise.all(ids.map((id) => api<void>(`/auth/sessions/${id}`, { method: "DELETE" }))),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sessions"] }),
  });
  const revokeOthers = useMutation({
    mutationFn: () => api<{ revoked: number }>("/auth/sessions", { method: "DELETE" }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sessions"] }),
  });
  const others = data ? data.items.filter((x) => !x.current).length : 0;
  const device = (ua: string | null) => {
    const { browser, os } = parseUserAgent(ua);
    if (browser && os) return t("sessions.browser_on_os", { browser, os });
    return browser ?? os ?? t("sessions.unknown_device");
  };
  return (
    <div className="flex flex-col gap-6">
    <PageHeader title={t("nav.account")} />
    <ProfilePictureCard />
    <PasswordChangeCard />
    <TwoFactorCard />
    <PasskeysCard />
    <SecurityQuestionCard />
    <LinkedAccountsCard />
    <ConsentCard />
    <PrivacyCard />
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-2">
          <CardTitle>{t("sessions.title")}</CardTitle>
          {others > 0 && <Button size="sm" variant="outline" className="text-destructive hover:text-destructive" disabled={revokeOthers.isPending} onClick={() => revokeOthers.mutate()}>{t("sessions.revoke_others")}</Button>}
        </CardHeader>
      </Card>
      <ListCard<SessionGroup>
        rows={data ? groupSessions(data.items) : undefined} rowKey={(s) => s.id} loading={isLoading}
        columns={[
          { header: t("sessions.device"), primary: true, cell: (s) => <span title={s.user_agent ?? undefined} className="inline-flex flex-wrap items-center gap-1.5">{device(s.user_agent)} {s.current && <Badge variant="info">{t("sessions.current")}</Badge>}{s.ids.length > 1 && <Badge variant="muted">{t("sessions.count", { count: s.ids.length })}</Badge>}</span> },
          { header: t("sessions.ip"), cell: (s) => <span className="font-mono text-xs">{s.ip ?? "-"}</span> },
          { header: t("sessions.last_seen"), cell: (s) => formatDateTime(s.last_seen_at, lang) },
          { header: "", className: "text-right", cell: (s) => !s.current && <Button size="sm" variant="outline" className="text-destructive hover:text-destructive" disabled={revoke.isPending} onClick={() => revoke.mutate(s.ids)}>{t("sessions.revoke")}</Button> },
        ]}
      />
    </div>
    </div>
  );
}
