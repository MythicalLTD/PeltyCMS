import { ArrowFatLinesUpIcon } from "@phosphor-icons/react";
import { Badge } from "@/components/ui/badge";
import { formatMoney } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import type { UpgradeCredit } from "@/lib/tiers";

/** "Upgrade from VIP: −€9.99" on a package the viewer can upgrade to. */
export function UpgradeBadge({ credit, currency }: { credit: UpgradeCredit | undefined; currency: string }) {
  const { t, lang } = useI18n();
  if (!credit) return null;
  return (
    <Badge variant="secondary" className="mt-1 gap-1" data-testid="upgrade-badge">
      <ArrowFatLinesUpIcon aria-hidden />
      {t("tiers.badge", { from: credit.from_name, amount: formatMoney(credit.amount, currency, lang) })}
    </Badge>
  );
}
