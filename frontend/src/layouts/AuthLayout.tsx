import { Outlet } from "react-router";
import { ShieldCheckIcon, LightningIcon, ReceiptIcon } from "@phosphor-icons/react";
import { Logo } from "@/components/Logo";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";
import { useI18n } from "@/lib/i18n";

/**
 * Based on the shadcn `login-02` block: form column on the left, brand panel
 * on the right (lg+). Phones get the form only.
 */
export function AuthLayout() {
  const { t } = useI18n();
  const points = [
    { icon: LightningIcon, title: t("auth.hero.p1_title"), text: t("auth.hero.p1_text") },
    { icon: ReceiptIcon, title: t("auth.hero.p2_title"), text: t("auth.hero.p2_text") },
    { icon: ShieldCheckIcon, title: t("auth.hero.p3_title"), text: t("auth.hero.p3_text") },
  ];
  return (
    <div className="grid min-h-svh lg:grid-cols-2">
      <div className="flex flex-col gap-4 p-4 md:p-8">
        <header className="flex items-center justify-between gap-3">
          <Logo />
          <LanguageSwitcher />
        </header>
        <main className="flex flex-1 items-center justify-center py-6 md:pb-20">
          {/* Keep Card chrome on every auth page (login wraps Card in ThemeRegion). */}
          <div className="w-full max-w-md">
            <Outlet />
          </div>
        </main>
      </div>
      <aside className="relative hidden flex-col justify-center gap-10 overflow-hidden border-l bg-card p-12 text-card-foreground lg:flex">
        <div className="relative flex max-w-md flex-col gap-3">
          <h2 className="text-3xl font-semibold tracking-tight text-balance text-foreground">{t("auth.hero.title")}</h2>
          <p className="text-muted-foreground">{t("auth.hero.subtitle")}</p>
        </div>
        <ul className="relative flex max-w-md flex-col gap-5">
          {points.map(({ icon: I, title, text }) => (
            <li key={title} className="flex gap-4">
              <div className="flex size-11 shrink-0 items-center justify-center rounded-xl bg-primary/15 text-primary"><I weight="duotone" className="size-6" aria-hidden /></div>
              <div><p className="font-medium text-foreground">{title}</p><p className="text-sm text-muted-foreground">{text}</p></div>
            </li>
          ))}
        </ul>
      </aside>
    </div>
  );
}
