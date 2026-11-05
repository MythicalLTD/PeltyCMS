import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";

export interface GeoCountry { code: string; name: string }
export interface GeoState { code: string; name: string; country: string }

const MONTH_MS = 30 * 24 * 60 * 60 * 1000;

export function useCountries() {
  return useQuery({
    queryKey: ["public", "geo", "countries"],
    queryFn: () => api<{ items: GeoCountry[] }>("/public/geo/countries"),
    staleTime: MONTH_MS,
    gcTime: MONTH_MS,
  });
}

export function useStates(country: string) {
  const code = country.trim().toUpperCase();
  const ok = /^[A-Z]{2}$/.test(code);
  return useQuery({
    queryKey: ["public", "geo", "states", code],
    queryFn: () => api<{ country: string; items: GeoState[] }>(`/public/geo/states?country=${code}`),
    enabled: ok,
    staleTime: MONTH_MS,
    gcTime: MONTH_MS,
  });
}
