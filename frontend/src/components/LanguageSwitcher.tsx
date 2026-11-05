import { TranslateIcon } from "@phosphor-icons/react";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useI18n } from "@/lib/i18n";
import { useMe } from "@/lib/auth";
import { api, type Me } from "@/lib/api";
import { useQueryClient } from "@tanstack/react-query";

export function LanguageSwitcher() {
  const { lang, languages, setLang, t } = useI18n();
  const { data: me } = useMe();
  const qc = useQueryClient();
  const persist = async (x: string) => {
    await api<void>("/auth/locale", { method: "PUT", body: { locale: x } });
    qc.setQueryData<Me | null>(["me"], (m) => (m ? { ...m, locale: x } : m));
  };
  return (
    <Select value={lang} onValueChange={(l) => void setLang(l, me ? persist : undefined)}>
      <SelectTrigger className="h-11 shrink-0 md:h-8" aria-label={t("nav.language")} data-testid="language-switcher">
        <TranslateIcon aria-hidden />
        <span className="uppercase sm:hidden">{lang}</span>
        <span className="hidden sm:inline-flex"><SelectValue /></span>
      </SelectTrigger>
      <SelectContent align="end">
        {languages.map((l) => <SelectItem key={l.code} value={l.code} className="min-h-11 md:min-h-8">{l.name}</SelectItem>)}
      </SelectContent>
    </Select>
  );
}
