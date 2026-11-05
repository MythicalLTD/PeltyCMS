import { useSearchParams } from "react-router";
import { PageHeader } from "@/components/PageHeader";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useI18n } from "@/lib/i18n";
import { AdminAddonsPage } from "@/pages/admin/extensions/Addons";
import { AdminPluginsPage } from "@/pages/admin/extensions/Plugins";

export type ExtensionsTab = "modules" | "hooks";

/** One admin home for installable modules (WASM) and hook/webhook plugins. */
export function AdminExtensionsPage() {
  const { t } = useI18n();
  const [params, setParams] = useSearchParams();
  const tab: ExtensionsTab = params.get("tab") === "hooks" ? "hooks" : "modules";
  return (
    <div className="flex min-w-0 flex-col gap-4">
      <PageHeader title={t("extensions.title")} description={t("extensions.subtitle")} />
      <Tabs
        value={tab}
        onValueChange={(v) => {
          const next = new URLSearchParams(params);
          if (v === "hooks") next.set("tab", "hooks");
          else next.delete("tab");
          setParams(next, { replace: true });
        }}
      >
        <TabsList>
          <TabsTrigger value="modules">{t("extensions.tab.modules")}</TabsTrigger>
          <TabsTrigger value="hooks">{t("extensions.tab.hooks")}</TabsTrigger>
        </TabsList>
        <TabsContent value="modules" className="pt-2">
          <AdminAddonsPage embedded />
        </TabsContent>
        <TabsContent value="hooks" className="pt-2">
          <AdminPluginsPage embedded />
        </TabsContent>
      </Tabs>
    </div>
  );
}
