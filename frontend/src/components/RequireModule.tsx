import type { ReactNode } from "react";
import { NotFoundPage } from "@/pages/NotFound";
import { PageSkeleton } from "@/components/PageSkeleton";
import { useModules, type ModuleId } from "@/lib/modules";

/** Renders children only while the feature module is enabled (404 otherwise). */
export function RequireModule({ id, children }: { id: ModuleId; children: ReactNode }) {
  const { enabled, loaded } = useModules();
  if (!loaded) return <PageSkeleton />;
  return enabled.has(id) ? <>{children}</> : <NotFoundPage />;
}
