import { useQuery } from "@tanstack/react-query";
import { PageHeader } from "@/components/PageHeader";
import { ArrowSquareOutIcon, WarningIcon } from "@phosphor-icons/react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { useI18n } from "@/lib/i18n";

interface DocPage { slug: string; title: string; section: string }

/** Long sections (one page per bundled addon) only show this many links here. */
const MAX_LINKS = 12;

/**
 * Entry point to the generated docs served by the backend at `/docs/`:
 * the user docs at `/docs/` and the developer docs at `/docs/developers/`.
 */
export function DeveloperDocsPage() {
  const { t } = useI18n();
  const { data, isLoading, isError } = useQuery({
    queryKey: ["dev-docs-pages"],
    queryFn: async () => {
      const r = await fetch("/docs/developers/pages.json", { credentials: "same-origin" });
      if (!r.ok) throw new Error(String(r.status));
      return (await r.json()) as DocPage[];
    },
    retry: false,
  });
  const built = !!data && data.length > 0;
  const sections = [...new Set((data ?? []).map((p) => p.section))];
  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title={t("nav.developer_docs")}
        description={t("devdocs.intro")}
        actions={built && <>
          <Button asChild variant="outline"><a href="/docs/" target="_blank" rel="noopener">{t("devdocs.user_docs")}<ArrowSquareOutIcon /></a></Button>
          <Button asChild><a href="/docs/developers/" target="_blank" rel="noopener">{t("devdocs.open")}<ArrowSquareOutIcon /></a></Button>
        </>}
      />
      {(isError || (data && !built)) && (
        <Alert><WarningIcon /><AlertDescription>{t("devdocs.not_built")}</AlertDescription></Alert>
      )}
      {isLoading && <Skeleton className="h-40 w-full" />}
      {built && (
        <div className="grid gap-4 md:grid-cols-2">
          {sections.map((s) => {
            const pages = data.filter((p) => p.section === s);
            return (
              <Card key={s}>
                <CardHeader>
                  <CardTitle>{s}</CardTitle>
                  <CardDescription>{pages.length}</CardDescription>
                </CardHeader>
                <CardContent>
                  <ul className="grid gap-1 text-sm">
                    {pages.slice(0, MAX_LINKS).map((p) => (
                      <li key={p.slug}><a className="text-primary hover:underline" href={`/docs/developers/${p.slug}.html`} target="_blank" rel="noopener">{p.title}</a></li>
                    ))}
                    {pages.length > MAX_LINKS && (
                      <li><a className="text-muted-foreground hover:underline" href={`/docs/developers/${pages[0]?.slug ?? "index"}.html`} target="_blank" rel="noopener">{t("devdocs.more", { n: pages.length - MAX_LINKS })}</a></li>
                    )}
                  </ul>
                </CardContent>
              </Card>
            );
          })}
        </div>
      )}
    </div>
  );
}
