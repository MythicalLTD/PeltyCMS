import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { api, ApiError, setCsrfToken, type Me } from "./api";
import { bindCache, clearCache } from "./query-cache";

export function useMe() {
  const qc = useQueryClient();
  const q = useQuery({
    queryKey: ["me"],
    queryFn: async () => {
      try {
        const me = await api<Me>("/auth/me");
        setCsrfToken(me.csrf_token);
        return me;
      } catch (e) {
        if (e instanceof ApiError && e.status === 401) {
          setCsrfToken(null);
          return null;
        }
        throw e;
      }
    },
    staleTime: 60_000,
    retry: false,
  });
  const uid = q.isSuccess ? (q.data?.id ?? null) : undefined;
  useEffect(() => {
    if (uid !== undefined) void bindCache(qc, uid);
  }, [qc, uid]);
  return q;
}

export type LoginResult = { me: Me; challenge?: undefined; duo?: undefined; methods?: undefined } | { challenge: string; duo?: boolean; methods: string[]; me?: undefined };

export function useAuthActions() {
  const qc = useQueryClient();
  return {
    /** Password step. Returns the user, or a challenge when 2FA is on. */
    async login(email: string, password: string): Promise<LoginResult> {
      const r = await api<Me | { mfa_required: true; duo_required?: boolean; challenge: string; methods?: string[] }>("/auth/login", { method: "POST", body: { email, password } });
      if ("mfa_required" in r) return { challenge: r.challenge, duo: r.duo_required === true, methods: r.methods ?? ["totp"] };
      setCsrfToken(r.csrf_token);
      qc.setQueryData(["me"], r);
      return { me: r };
    },
    /** Second step: a 6-digit TOTP code or a backup code. */
    async verifyMfa(challenge: string, code: string) {
      const me = await api<Me>("/auth/mfa/verify", { method: "POST", body: { challenge, code } });
      setCsrfToken(me.csrf_token);
      qc.setQueryData(["me"], me);
      return me;
    },
    async logout() {
      // SAML-aware logout: ends the session and returns the IdP SLO URL when the session came from one.
      const r = await api<{ url?: string }>("/auth/saml/logout", { method: "POST" }).catch(() => null);
      if (!r) await api<void>("/auth/logout", { method: "POST" }).catch(() => undefined);
      setCsrfToken(null);
      await clearCache();
      qc.setQueryData(["me"], null);
      qc.removeQueries({ predicate: (q) => q.queryKey[0] !== "me" });
      if (r?.url && /^https:\/\//.test(r.url)) window.location.assign(r.url);
    },
  };
}

export const isStaff = (me: Me | null | undefined) => !!me?.permissions.includes("admin.access");
