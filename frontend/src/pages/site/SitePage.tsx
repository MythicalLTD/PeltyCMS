import { useEffect } from "react";
import { PageSkeleton } from "@/components/PageSkeleton";
import { Navigate, useParams } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { Blocks } from "@/components/site/Blocks";
import { NotFoundPage } from "@/pages/NotFound";
import { api, ApiError, isUnreachableError } from "@/lib/api";
import { useMe } from "@/lib/auth";
import { usePublicSite, type Block } from "@/lib/admin/site";
import { usePublicMode } from "@/lib/publicMode";
import { lazy, Suspense } from "react";

const StoreHomePage = lazy(() => import("@/pages/site/StoreHome").then((m) => ({ default: m.StoreHomePage })));
const MarketplaceHomePage = lazy(() => import("@/pages/site/Marketplace").then((m) => ({ default: m.MarketplaceHomePage })));

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

/**
 * `/`: the game store or marketplace home when that look is active (for
 * everyone), else the hosting landing for visitors and the dashboard for
 * signed-in users.
 */
export function HomeGate() {
  const { data: me, isLoading } = useMe();
  const { mode, loading } = usePublicMode();
  if (isLoading || loading) return <PageSkeleton className="p-8" />;
  if (mode === "store") return <Suspense fallback={<PageSkeleton className="p-8" />}><StoreHomePage /></Suspense>;
  if (mode === "marketplace") return <Suspense fallback={<PageSkeleton className="p-8" />}><MarketplaceHomePage /></Suspense>;
  if (me) return <Navigate to="/dashboard" replace />;
  return <SitePage />;
}
