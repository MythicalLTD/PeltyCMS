import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api } from "./api";
import { baseMessages, pickLanguage, readCookie, translate, translateCode, type Messages, type TranslationKey } from "./i18n-core";

export interface Language {
  code: string;
  name: string;
}

interface I18nValue {
  lang: string;
  languages: Language[];
  t: (key: TranslationKey, vars?: Record<string, string | number>) => string;
  /** Translate a server-provided message code (e.g. "error.forbidden"). */
  tCode: (code: string) => string;
  setLang: (lang: string, persist?: (lang: string) => Promise<void>) => Promise<void>;
}

const Ctx = createContext<I18nValue | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState("en");
  const [languages, setLanguages] = useState<Language[]>([{ code: "en", name: "English" }]);
  const [messages, setMessages] = useState<Messages>(baseMessages);

  const seq = useRef(0);
  const load = useCallback(async (l: string) => {
    // Server catalog includes plugin/theme namespaces; bundled en is the fallback.
    // Only the latest request may apply (no out-of-order flips).
    const n = ++seq.current;
    const m = await api<Messages>(`/i18n/${encodeURIComponent(l)}`).catch(() => ({}) as Messages);
    if (n !== seq.current) return;
    setMessages({ ...baseMessages, ...m });
    setLangState(l);
    document.documentElement.lang = l;
  }, []);

  useEffect(() => {
    const boot = () =>
      api<{ languages: Language[] }>("/i18n")
        .then((r) => {
          setLanguages(r.languages);
          const l = pickLanguage(r.languages.map((x) => x.code), readCookie("pelty_lang", document.cookie), navigator.languages);
          return load(l);
        })
        .catch(() => undefined);
    void boot();
    const onChange = () => {
      void api<{ languages: Language[] }>("/i18n")
        .then((r) => setLanguages(r.languages))
        .catch(() => undefined);
    };
    window.addEventListener("pelty:languages-changed", onChange);
    return () => window.removeEventListener("pelty:languages-changed", onChange);
  }, [load]);

  const value = useMemo<I18nValue>(
    () => ({
      lang,
      languages,
      t: (key, vars) => translate(messages, key, vars, lang),
      tCode: (code) => translateCode(messages, code),
      setLang: async (l, persist) => {
        document.cookie = `pelty_lang=${encodeURIComponent(l)}; Path=/; Max-Age=31536000; SameSite=Lax`;
        if (persist) await persist(l).catch(() => undefined);
        await load(l);
      },
    }),
    [lang, languages, messages, load],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useI18n(): I18nValue {
  const v = useContext(Ctx);
  if (!v) throw new Error("useI18n outside I18nProvider");
  return v;
}
