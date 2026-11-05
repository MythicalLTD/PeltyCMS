import { Navigate, useLocation } from "react-router";
import { pathAllowed, useClientAccess } from "@/lib/clientAccess";
import { PageSkeleton } from "@/components/PageSkeleton";

/** Blocks client routes the signed-in contact cannot access. */
export function ClientAccessGate({ children }: { children: React.ReactNode }) {
  const loc = useLocation();
  const access = useClientAccess();
  if (access.isLoading) return <PageSkeleton />;
  if (access.data && !pathAllowed(loc.pathname, access.data)) return <Navigate to="/dashboard" replace />;
  return children;
}
