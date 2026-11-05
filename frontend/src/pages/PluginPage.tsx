import { useParams } from "react-router";
import { PluginView } from "@/components/plugin-ui/PluginRenderer";
import { usePluginPages } from "@/lib/addons/plugins";
import { PageSkeleton } from "@/components/PageSkeleton";
import { NotFoundPage } from "@/pages/NotFound";
import { PageHeader } from "@/components/PageHeader";
import { useI18n } from "@/lib/i18n";

/** Renders a plugin's page (declarative tree or sandboxed addon frame) at /admin/p/:plugin/:page or /p/:plugin/:page. */
export function PluginPage({ area }: { area: "admin" | "client" }) {
  const { plugin, page } = useParams();
  const { tCode } = useI18n();
  const { pages, loading } = usePluginPages(area);
  const p = pages.find((x) => x.plugin === plugin && x.id === page);
  if (loading) return <PageSkeleton />;
  if (!p) return <NotFoundPage />;
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={tCode(p.title)} />
      <PluginView key={`${p.plugin}/${p.id}`} plugin={p.plugin} data={p.data} body={p.body} frame={p.frame} bridge={p.bridge} sameOrigin={p.same_origin} base={{ ctx: {}, params: {} }} />
    </div>
  );
}

export function AdminPluginPage() {
  return <PluginPage area="admin" />;
}
export function ClientPluginPage() {
  return <PluginPage area="client" />;
}
