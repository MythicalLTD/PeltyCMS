import { toneVariant } from "@/lib/utils";
import { Badge } from "./ui/badge";
import { statusTone } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";

export function StatusBadge({ status }: { status: string }) {
  const { tCode } = useI18n();
  return <Badge variant={toneVariant(statusTone(status))}>{tCode(`status.${status}`)}</Badge>;
}
