import { PageHeader } from "@/components/PageHeader";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { api } from "@/lib/api";
import { loadActiveTheme, type Theme } from "@/lib/theme";
import { useI18n } from "@/lib/i18n";

export function AdminThemesPage() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["themes"], queryFn: () => api<{ items: Theme[]; active: string }>("/themes") });
  const activate = useMutation({
    mutationFn: (id: string) => api<void>("/admin/themes/active", { method: "PUT", body: { id } }),
    onSuccess: async () => {
      // Restyle in place: apply the new tokens/stylesheet, then refresh
      // everything that reads the active theme (logo, list badge).
      const th = await loadActiveTheme();
      qc.setQueryData(["theme", "active"], th);
      await qc.invalidateQueries({ queryKey: ["theme"] });
      await qc.invalidateQueries({ queryKey: ["themes"] });
    },
  });
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("admin.themes.title")} />
      <div className="grid gap-4 *:min-w-0 sm:grid-cols-2 lg:grid-cols-3">
        {data?.items.map((th) => (
          <Card key={th.id} className="gap-0 overflow-hidden py-0 shadow-sm">
            <ThemePreview th={th} />
            <CardContent className="flex flex-1 flex-col gap-3 p-5">
              <div>
                <p className="font-medium">{th.name} <span className="font-mono text-xs text-muted-foreground">v{th.version}</span></p>
                <p className="text-xs text-muted-foreground">{t("admin.themes.by", { author: th.author })}</p>
                {th.description && <p className="mt-2 line-clamp-2 text-sm text-muted-foreground">{th.description}</p>}
              </div>
              {data.active === th.id ? (
                <Badge variant="success" className="mt-auto self-start">{t("admin.themes.active")}</Badge>
              ) : (
                <Button size="sm" variant="outline" className="mt-auto min-h-11 self-start sm:min-h-8" disabled={activate.isPending} onClick={() => activate.mutate(th.id)}>{t("admin.themes.activate")}</Button>
              )}
            </CardContent>
          </Card>
        ))}
      </div>
    </div>
  );
}

/** Mini mock-up of the app chrome (sidebar, card, button) painted with the theme's own tokens. */
function ThemePreview({ th }: { th: Theme }) {
  const dark = th.color_scheme === "dark";
  const tk = (k: string, fb: string) => th.tokens[k] ?? fb;
  const bg = tk("background", dark ? "#0b0b0c" : "#ffffff");
  const fg = tk("foreground", dark ? "#f5f5f5" : "#111111");
  const card = tk("card", tk("surface", dark ? "#16161a" : "#ffffff"));
  const border = tk("border", dark ? "#2a2a2e" : "#e5e5e5");
  const muted = tk("muted-foreground", dark ? "#9a9aa2" : "#71717a");
  const primary = tk("primary", "#2f6fed");
  const side = tk("sidebar", tk("muted", dark ? "#111114" : "#f4f4f5"));
  const radius = tk("radius", "0.5rem");
  return (
    <div className="flex h-32 gap-2 overflow-hidden rounded-t-xl border-b p-2" style={{ background: bg, color: fg, borderColor: border, fontFamily: th.tokens["font-sans"] }} aria-hidden>
      <div className="flex w-1/4 flex-col gap-1.5 p-2" style={{ background: side, borderRadius: radius }}>
        <span className="h-2 w-3/4 rounded-full" style={{ background: primary }} />
        {[0, 1, 2].map((i) => <span key={i} className="h-1.5 rounded-full" style={{ background: muted, opacity: 0.5 }} />)}
      </div>
      <div className="flex flex-1 flex-col gap-2 p-3" style={{ background: card, border: `1px solid ${border}`, borderRadius: radius }}>
        <span className="text-lg leading-none font-semibold">Aa</span>
        <span className="h-1.5 w-2/3 rounded-full" style={{ background: muted, opacity: 0.6 }} />
        <span className="h-1.5 w-1/2 rounded-full" style={{ background: muted, opacity: 0.4 }} />
        <span className="mt-auto h-4 w-16" style={{ background: primary, borderRadius: radius }} />
      </div>
    </div>
  );
}
