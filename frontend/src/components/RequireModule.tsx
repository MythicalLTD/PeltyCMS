import type { ReactNode } from "react";
import { NotFoundPage } from "@/pages/NotFound";
import { PageSkeleton } from "@/components/PageSkeleton";
import { useModules, type ModuleId } from "@/lib/modules";
import { useI18n } from "@/lib/i18n";

/** Renders children only while the feature module is enabled (404 otherwise). */
export function RequireModule({ id, children }: { id: ModuleId; children: ReactNode }) {
  const { t } = useI18n();
  const { enabled, loaded, failed } = useModules();
  // A failed lookup is not a "disabled" answer: never show 404 for it.
  if (failed) return <p role="alert" className="p-6 text-sm text-destructive">{t("error.internal")}</p>;
  if (!loaded) return <PageSkeleton />;
  return enabled.has(id) ? <>{children}</> : <NotFoundPage />;
}
