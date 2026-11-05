import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowClockwiseIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, setCaptchaToken } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { widgetFor, type CaptchaConfig } from "@/lib/captcha";

export type { CaptchaConfig, CaptchaWidget } from "@/lib/captcha";

type Widget = { render: (el: HTMLElement, o: Record<string, unknown>) => string | number; reset: (id?: string | number) => void };
type V3 = { ready: (cb: () => void) => void; execute: (key: string, o: { action: string }) => Promise<string> };
const win = window as unknown as Record<string, unknown>;

function loadScript(src: string): Promise<void> {
  const existing = document.querySelector<HTMLScriptElement>(`script[src="${src}"]`);
  if (existing) return existing.dataset.loaded ? Promise.resolve() : new Promise((r) => existing.addEventListener("load", () => r()));
  return new Promise((resolve, reject) => {
    const el = document.createElement("script");
    el.src = src; el.async = true;
    el.addEventListener("load", () => { el.dataset.loaded = "1"; resolve(); }, { once: true });
    el.addEventListener("error", () => reject(new Error("captcha script")), { once: true });
    document.head.appendChild(el);
  });
}

export function useCaptchaConfig() {
  return useQuery({ queryKey: ["captcha-config"], queryFn: () => api<CaptchaConfig>("/captcha/config"), staleTime: 60_000 });
}

/** Renders the configured captcha for `form` (nothing when not required). Sets the one-shot token used by the next API call. */
export function Captcha({ form, signedIn = false }: { form: string; signedIn?: boolean }) {
  const { t } = useI18n();
  const cfg = useCaptchaConfig().data;
  const active = !!cfg?.enabled && cfg.forms.includes(form) && !(cfg.mode === "guests" && signedIn);
  const engine = cfg?.engine ?? "";
  const key = cfg?.site_key ?? "";
  const widget = widgetFor(cfg);
  const script = widget?.script ?? "";
  const global = widget?.global ?? "";
  const score = widget?.mode === "score";
  const box = useRef<HTMLDivElement>(null);
  const wid = useRef<string | number | null>(null);
  const goneRef = useRef(false);
  const [challenge, setChallenge] = useState<{ token: string; question: string } | null>(null);
  const [answer, setAnswer] = useState("");

  const refresh = useCallback(async () => {
    setCaptchaToken(null);
    if (!active) return;
    if (engine === "internal") {
      setAnswer("");
      setChallenge(await api<{ token: string; question: string }>("/captcha/challenge"));
    } else if (score && script) {
      await loadScript(script);
      const g = win[global] as V3;
      g.ready(() => { void g.execute(key, { action: form }).then((tok) => { setCaptchaToken(tok); return tok; }); });
    } else {
      const lib = win[global] as Widget | undefined;
      if (lib && wid.current !== null) lib.reset(wid.current);
    }
  }, [active, engine, key, form, score, script, global]);

  useEffect(() => {
    if (!active) return;
    goneRef.current = false;
    if (script && !score) {
      void loadScript(script).then(() => {
        const tryRender = () => {
          const lib = win[global] as Widget | undefined;
          if (goneRef.current || !box.current) return;
          if (!lib?.render) { setTimeout(tryRender, 100); return; }
          if (wid.current === null) wid.current = lib.render(box.current, { sitekey: key, callback: (tok: string) => setCaptchaToken(tok), "expired-callback": () => setCaptchaToken(null) });
        };
        tryRender();
        return undefined;
      });
    } else {
      void (async () => {
        if (engine === "internal") {
          const ch = await api<{ token: string; question: string }>("/captcha/challenge");
          if (goneRef.current) return;
          setAnswer("");
          setChallenge(ch);
        } else if (score && script) {
          await loadScript(script);
          if (goneRef.current) return;
          const g = win[global] as V3;
          await new Promise<void>((resolve) => g.ready(resolve));
          if (goneRef.current) return;
          setCaptchaToken(await g.execute(key, { action: form }));
        } else {
          setCaptchaToken(null);
        }
      })();
    }
    const onReset = () => { void refresh(); };
    window.addEventListener("pelty:captcha-reset", onReset);
    return () => { goneRef.current = true; window.removeEventListener("pelty:captcha-reset", onReset); setCaptchaToken(null); };
  }, [active, engine, key, form, refresh, script, score, global]);

  if (!active) return null;
  if (score) return <p className="text-xs text-muted-foreground">{t("captcha.recaptcha_notice")}</p>;
  if (engine === "internal") {
    return (
      <div className="flex flex-col gap-2">
        <Label htmlFor={`captcha-${form}`}>{t("captcha.question")} <span className="font-mono">{challenge?.question ?? "…"} = ?</span></Label>
        <div className="flex gap-2">
          <Input id={`captcha-${form}`} inputMode="numeric" autoComplete="off" value={answer} onChange={(e) => { setAnswer(e.target.value); setCaptchaToken(challenge && e.target.value ? `${challenge.token}:${e.target.value.trim()}` : null); }} />
          <Button type="button" variant="outline" size="icon" aria-label={t("captcha.new")} onClick={() => void refresh()}><ArrowClockwiseIcon /></Button>
        </div>
      </div>
    );
  }
  return <div ref={box} className="min-h-16 max-w-full overflow-x-auto" />;
}
