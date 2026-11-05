import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";

export interface ClientAccess {
  owner: boolean;
  services: boolean;
  invoices: boolean;
  tickets: boolean;
  orders: boolean;
  domains: boolean;
}

/** Whether a client-area path is allowed for this access grant. */
export function pathAllowed(path: string, a: ClientAccess | undefined): boolean {
  if (!a || a.owner) return true;
  if (path.startsWith("/services") || path.startsWith("/store") || path === "/order" || path.startsWith("/downloads")) return a.services;
  if (path.startsWith("/invoices") || path.startsWith("/billing") || path.startsWith("/quotes")) return a.invoices;
  if (path.startsWith("/tickets") || path.startsWith("/projects")) return a.tickets;
  if (path.startsWith("/orders")) return a.orders;
  if (path.startsWith("/domains") || path.startsWith("/ssl")) return a.domains;
  if (path.startsWith("/affiliate")) return false;
  return true;
}

export function useClientAccess(enabled = true) {
  return useQuery({
    queryKey: ["client", "access"],
    queryFn: () => api<ClientAccess>("/client/access"),
    staleTime: 60_000,
    enabled,
  });
}
