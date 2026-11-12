import { CheckCircleIcon, GameControllerIcon } from "@phosphor-icons/react";
import { Link } from "react-router";
import { Field } from "@/components/Field";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { identityError } from "@/lib/guestCheckout";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";
import { askedFields, identityDisplay, type IdentityGame } from "@/lib/store/checkoutFields";

type Typed = Record<string, Record<string, string>>;

/** Typed player details with a format problem (`game.field`). */
export function badIdentity(games: IdentityGame[], typed: Typed): string[] {
  return games.flatMap((g) => askedFields(g).filter((f) => identityError(g.game, f.field, typed[g.game]?.[f.field] ?? "")).map((f) => `${g.game}.${f.field}`));
}

/**
 * Signed-in checkout, game store: the IDs we already have (from Login with
 * FiveM / Steam / Discord or an earlier purchase) shown as filled, and an
 * input for each one still missing that the package needs (the Rockstar
 * licence on FiveM/RedM is always asked when missing).
 */
export function PlayerDetailsCard({ games, typed, onChange, shown, missing }: {
  games: IdentityGame[]; typed: Typed; onChange: (game: string, field: string, value: string) => void; shown: boolean; missing: string[];
}) {
  const { t } = useI18n();
  const bad = badIdentity(games, typed);
  const visible = games.filter((g) => g.fields.some((f) => f.value) || askedFields(g).length > 0);
  if (visible.length === 0) return null;
  const label = (f: string) => t(`game.field.${f}` as TranslationKey);
  return (
    <Card data-testid="player-details">
      <CardHeader>
        <CardTitle className="flex items-center gap-2"><GameControllerIcon aria-hidden className="size-5 text-primary" />{t("pid.title")}</CardTitle>
        <CardDescription>{t("pid.desc")}</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-5">
        {visible.map((g) => {
          const known = g.fields.filter((f) => f.value);
          const asked = askedFields(g);
          return (
            <div key={g.game} className="grid gap-3">
              {visible.length > 1 && <p className="text-sm font-medium">{g.name}</p>}
              {known.length > 0 && (
                <ul className="grid gap-2 sm:grid-cols-2" aria-label={t("pid.known")}>
                  {known.map((f) => (
                    <li key={f.field} className="flex min-w-0 items-center gap-2 rounded-md border bg-muted/40 px-3 py-2" data-known={f.field}>
                      <CheckCircleIcon aria-hidden weight="fill" className="size-4 shrink-0 text-primary" />
                      <span className="min-w-0 flex-1"><span className="block text-xs text-muted-foreground">{label(f.field)}</span><span className="block truncate font-mono text-sm">{identityDisplay(f.field, f.value ?? "")}</span></span>
                      <Badge variant="secondary" className="shrink-0">{t("pid.filled")}</Badge>
                    </li>
                  ))}
                </ul>
              )}
              {asked.length > 0 && (
                <div className="grid gap-3 *:min-w-0 sm:grid-cols-2">
                  {asked.map((f) => {
                    const k = `${g.game}.${f.field}`;
                    const err = shown && bad.includes(k) ? t("error.game_identity_invalid") : shown && missing.includes(k) ? t("store.field_required") : undefined;
                    return (
                      <Field key={k} id={`pid-${g.game}-${f.field}`} label={`${label(f.field)}${f.required ? " *" : ""}`} autoComplete="off" spellCheck={false}
                        className="font-mono" required={f.required} placeholder={f.field === "license" ? "license:0a1b2c…" : undefined}
                        value={typed[g.game]?.[f.field] ?? ""} onChange={(e) => onChange(g.game, f.field, e.target.value)} error={err}
                        hint={t(`pid.hint.${f.field === "license" || f.field === "steamid" || f.field === "discord_id" || f.field === "fivem" ? f.field : "other"}` as TranslationKey)} />
                    );
                  })}
                </div>
              )}
            </div>
          );
        })}
        <p className="text-xs text-muted-foreground">{t("pid.saved_note")} <Link className="underline" to="/game-accounts">{t("pid.manage")}</Link></p>
      </CardContent>
    </Card>
  );
}
