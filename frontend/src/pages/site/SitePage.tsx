import { useEffect } from "react";
import { PageSkeleton } from "@/components/PageSkeleton";
import { Navigate, useParams } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { Blocks } from "@/components/site/Blocks";
import { NotFoundPage } from "@/pages/NotFound";
import { api, ApiError, isUnreachableError } from "@/lib/api";
import { useMe } from "@/lib/auth";
import { usePublicSite, type Block } from "@/lib/admin/site";

interface PublicPage { slug: string; title: string; description: string; blocks: Block[] }

export function SitePage({ slug: fixed }: { slug?: string }) {
  const params = useParams();
  const site = usePublicSite();
  const slug = fixed ?? params.slug ?? site.data?.config.homepage ?? "home";
  const q = useQuery({
    queryKey: ["public", "page", slug],
    queryFn: () => api<PublicPage>(`/public/site/pages/${encodeURIComponent(slug)}`),
    enabled: !!slug && (!!fixed || !!params.slug || !site.isLoading),
    retry: (n, e) => !isUnreachableError(e) && !(e instanceof ApiError && e.status === 404) && n < 2,
  });
  useEffect(() => {
    if (q.data) document.title = `${q.data.title} · ${site.data?.config.site_name ?? "PeltyCMS"}`;
  }, [q.data, site.data]);
  if (q.isError) return <NotFoundPage />;
  if (!q.data) return <PageSkeleton className="p-8" />;
  return <Blocks blocks={q.data.blocks} />;
}

/** `/`: public homepage for visitors, client dashboard for signed-in users. */
export function HomeGate() {
  const { data: me, isLoading } = useMe();
  if (isLoading) return <PageSkeleton className="p-8" />;
  if (me) return <Navigate to="/dashboard" replace />;
  return <SitePage />;
}
