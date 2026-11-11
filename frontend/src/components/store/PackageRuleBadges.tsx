import { Badge } from "@/components/ui/badge";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";
import type { PublicRule } from "@/lib/storeRules";

/** Whether the rule stops this customer from buying now. */
export function ruleBlocks(r: PublicRule | undefined): boolean {
  return !!r && (r.global_left === 0 || r.user_left === 0);
}

/** Small badges for a package's store rules (limits, requirements, PWYW). */
export function PackageRuleBadges({ r }: { r: PublicRule | undefined }) {
  const { t } = useI18n();
  if (!r) return null;
  const period = (p: string) => t(`rules.period.${p}` as TranslationKey);
  const out: { key: string; text: string; tone?: "destructive" | "outline" }[] = [];
  if (r.global_left === 0) out.push({ key: "sold", text: t("rules.sold_out"), tone: "destructive" });
  else if (r.global_left != null && r.global_left <= 20) out.push({ key: "left", text: t("rules.only_left", { n: r.global_left }) });
  if (r.user_limit != null) out.push({ key: "user", text: r.user_left === 0 ? t("rules.limit_reached") : t("rules.limit_per_player", { n: r.user_limit, period: period(r.user_period) }), tone: r.user_left === 0 ? "destructive" : "outline" });
  if (r.required.length > 0) out.push({ key: "req", text: t("rules.requires", { names: r.required.map((x) => x.name).join(", ") }), tone: r.requirement_met === false ? "destructive" : "outline" });
  if (r.pay_what_you_want) out.push({ key: "pwyw", text: t("rules.pwyw") });
  if (out.length === 0) return null;
  return <div className="mt-1 flex flex-wrap gap-1" data-testid="package-rules-badges">{out.map((b) => <Badge key={b.key} variant={b.tone ?? "secondary"}>{b.text}</Badge>)}</div>;
}
