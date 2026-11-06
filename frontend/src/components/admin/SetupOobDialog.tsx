import { useMemo, useState } from "react";
import { Link } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  CheckCircleIcon,
  CircleIcon,
  CreditCardIcon,
  CurrencyCircleDollarIcon,
  EnvelopeSimpleIcon,
  GlobeIcon,
  BuildingsIcon,
  PackageIcon,
  RocketLaunchIcon,
  StorefrontIcon,
} from "@phosphor-icons/react";
import { toast } from "sonner";
import { ImageUrlField } from "@/components/admin/ImageUrlField";
import { LICENSE_KEY, useLicense, type LicenseStatus } from "@/components/admin/LicenseGate";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import type { GeneralSettings } from "@/lib/admin/general";
import { normalizeConfig, type SiteConfig } from "@/lib/admin/site";
import { buildConfig, gatewayFormFromConfig, type Gateway } from "@/pages/admin/billing/Gateways";
import { currencyPayload, type Currency } from "@/pages/admin/billing/Currencies";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";
import { requiredProgress, type Wizard } from "@/pages/admin/settings/GettingStarted";
import { slugify } from "@/pages/admin/catalog/ProductDialog";
import { cn } from "@/lib/utils";

const WIZARD_KEY = ["admin", "setup-wizard"];

/** Set when the site-branding step saves so reopening skips to gateway and product. */
const OOB_SITE_DONE_KEY = "pelty_oob_site_done";

type Phase = "license" | "company" | "currency" | "mail" | "site" | "next";

const PHASES: Phase[] = ["license", "company", "currency", "mail", "site", "next"];

const CURRENCY_PRESETS: Array<{ code: string; prefix: string; suffix: string; label: string }> = [
  { code: "EUR", prefix: "€", suffix: "", label: "Euro (EUR)" },
  { code: "USD", prefix: "$", suffix: "", label: "US Dollar (USD)" },
  { code: "GBP", prefix: "£", suffix: "", label: "Pound (GBP)" },
  { code: "RON", prefix: "", suffix: " lei", label: "Leu (RON)" },
];

interface SmtpForm {
  enabled: boolean;
  host: string;
  port: string;
  security: string;
  username: string;
  password: string;
  from_address: string;
  from_name: string;
}

function phaseIndex(p: Phase): number {
  return PHASES.indexOf(p);
}

function firstOpenPhase(w: Wizard): Phase {
  const done = (id: string) => !!w.steps.find((s) => s.id === id)?.done;
  if (!done("license")) return "license";
  if (!done("company")) return "company";
  if (!done("currency")) return "currency";
  if (!done("email")) return "mail";
  try {
    if (typeof localStorage !== "undefined" && localStorage.getItem(OOB_SITE_DONE_KEY) === "1") {
      return "next";
    }
  } catch {
    /* ignore */
  }
  return "site";
}

function StepRail({ phase }: { phase: Phase }) {
  const { t } = useI18n();
  const cur = phaseIndex(phase);
  return (
    <ol className="mb-1 flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground">
      {PHASES.map((p, i) => (
        <li
          key={p}
          className={cn(
            "flex items-center gap-1.5",
            i === cur && "font-medium text-foreground",
            i < cur && "text-primary",
          )}
        >
          <span
            className={cn(
              "inline-flex size-5 items-center justify-center rounded-full border text-[10px] tabular-nums",
              i < cur && "border-primary bg-primary text-primary-foreground",
              i === cur && "border-foreground",
            )}
          >
            {i < cur ? "✓" : i + 1}
          </span>
          {t(`setup.oob.phase.${p}` as TranslationKey)}
        </li>
      ))}
    </ol>
  );
}

/**
 * First-login OOB modal: license, company+logo, currency, SMTP, site branding,
 * then gateway and product shortcuts with deep links as fallback.
 */
export function SetupOobDialog() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const wizard = useQuery({
    queryKey: WIZARD_KEY,
    queryFn: () => api<Wizard>("/admin/setup-wizard"),
    retry: false,
  });
  const lic = useLicense();
  const [open, setOpen] = useState(false);
  const [phase, setPhase] = useState<Phase>("license");
  const [company, setCompany] = useState("");
  const [email, setEmail] = useState("");
  const [companyLogo, setCompanyLogo] = useState("");
  const [siteLogo, setSiteLogo] = useState("");
  const [domain, setDomain] = useState("");
  const [systemUrl, setSystemUrl] = useState("");
  const [currencyCode, setCurrencyCode] = useState("EUR");
  const [siteName, setSiteName] = useState("");
  const [tagline, setTagline] = useState("");
  const [smtp, setSmtp] = useState<SmtpForm>({
    enabled: true,
    host: "",
    port: "587",
    security: "starttls",
    username: "",
    password: "",
    from_address: "",
    from_name: "",
  });
  const [loaded, setLoaded] = useState(false);
  const [smtpHydrateKey, setSmtpHydrateKey] = useState<string | null>(null);
  const [siteHydrateKey, setSiteHydrateKey] = useState<string | null>(null);

  // Wait until the blocking license gate is gone so the two dialogs never stack.
  // Adjust open/phase during render (not in an effect) to avoid cascading commits.
  const wantOpen = !!wizard.data?.show && !lic.data?.show_accept_gate;
  if (wantOpen && !open) {
    setOpen(true);
    setPhase(firstOpenPhase(wizard.data!));
  } else if (!wantOpen && open) {
    setOpen(false);
  }

  const gs = useQuery({
    queryKey: ["admin", "general-settings"],
    queryFn: () => api<{ settings: GeneralSettings }>("/admin/general-settings"),
    enabled: open,
  });
  const currencies = useQuery({
    queryKey: ["admin", "currencies"],
    queryFn: () => api<{ items: Currency[] }>("/admin/currencies"),
    enabled: open && (phase === "currency" || phase === "next"),
  });
  const gatewaysQ = useQuery({
    queryKey: ["admin", "gateways"],
    queryFn: () => api<{ items: Gateway[] }>("/admin/gateways"),
    enabled: open && phase === "next",
  });
  const catalogGroupsQ = useQuery({
    queryKey: ["admin", "catalog", "groups"],
    queryFn: () => api<{ items: { id: string; name: string }[] }>("/admin/catalog/groups"),
    enabled: open && phase === "next",
  });
  const productsQ = useQuery({
    queryKey: ["admin", "products"],
    queryFn: () => api<{ items: { id: string }[] }>("/admin/catalog/products"),
    enabled: open && phase === "next",
  });
  const publicCurrenciesQ = useQuery({
    queryKey: ["currencies"],
    queryFn: () => api<{ items: { code: string }[] }>("/currencies"),
    enabled: open && phase === "next",
  });
  const smtpQ = useQuery({
    queryKey: ["admin", "settings", "smtp"],
    queryFn: () =>
      api<{
        enabled: boolean;
        host: string;
        port: number;
        security: string;
        username: string;
        password_set: boolean;
        from_address: string;
        from_name: string;
      }>("/admin/settings/smtp"),
    enabled: open && phase === "mail",
  });
  const siteQ = useQuery({
    queryKey: ["admin", "site"],
    queryFn: async () => normalizeConfig(await api<unknown>("/admin/site")),
    enabled: open && phase === "site",
  });

  if (gs.data && !loaded) {
    const g = gs.data.settings;
    setCompany(g.general.company_name);
    setEmail(g.general.email);
    setCompanyLogo(g.general.logo_url);
    setDomain(g.general.domain);
    setSystemUrl(g.system_urls.system_url);
    setLoaded(true);
  }

  const nextSmtpKey = smtpQ.data ? `${smtpQ.dataUpdatedAt}:${email}:${company}` : null;
  if (smtpQ.data && nextSmtpKey !== smtpHydrateKey) {
    const d = smtpQ.data;
    setSmtpHydrateKey(nextSmtpKey);
    setSmtp((prev) => ({
      ...prev,
      enabled: d.enabled,
      host: d.host,
      port: String(d.port || 587),
      security: d.security || "starttls",
      username: d.username,
      from_address: d.from_address || email,
      from_name: d.from_name || company,
    }));
  }

  const nextSiteKey = siteQ.data ? `${siteQ.dataUpdatedAt}:${companyLogo}` : null;
  if (siteQ.data && nextSiteKey !== siteHydrateKey) {
    setSiteHydrateKey(nextSiteKey);
    setSiteName(siteQ.data.site_name);
    setTagline(siteQ.data.tagline);
    setSiteLogo(siteQ.data.logo_url || companyLogo);
  }

  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));

  const refreshWizard = async () => {
    await qc.invalidateQueries({ queryKey: WIZARD_KEY });
  };

  const invalidateWizardAndRelated = async () => {
    await refreshWizard();
    await qc.invalidateQueries({ queryKey: ["admin", "gateways"] });
    await qc.invalidateQueries({ queryKey: ["admin", "products"] });
    await qc.invalidateQueries({ queryKey: ["admin", "catalog", "groups"] });
  };

  const acceptLic = useMutation({
    mutationFn: () =>
      api<LicenseStatus>("/admin/license/accept", {
        method: "POST",
        body: { accept: true },
      }),
    onSuccess: async (d) => {
      qc.setQueryData(LICENSE_KEY, d);
      await refreshWizard();
      toast.success(t("license.accepted"));
      setPhase("company");
    },
    onError: fail,
  });

  const saveCompany = useMutation({
    mutationFn: async () => {
      const cur = gs.data?.settings;
      if (!cur) throw new ApiError(500, "error.internal", "missing");
      const next: GeneralSettings = {
        ...cur,
        general: {
          ...cur.general,
          company_name: company.trim(),
          email: email.trim(),
          logo_url: companyLogo.trim(),
          domain: domain.trim(),
        },
        system_urls: {
          ...cur.system_urls,
          system_url: systemUrl.trim(),
        },
      };
      await api("/admin/general-settings", { method: "PUT", body: next });
      return next;
    },
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "general-settings"] });
      await qc.invalidateQueries({ queryKey: ["public", "general"] });
      await refreshWizard();
      toast.success(t("common.saved"));
      setPhase("currency");
    },
    onError: fail,
  });

  const saveCurrency = useMutation({
    mutationFn: async () => {
      const items = currencies.data?.items ?? [];
      const existing = items.find((c) => c.code === currencyCode);
      if (existing?.enabled) return existing;
      const preset = CURRENCY_PRESETS.find((p) => p.code === currencyCode) ?? CURRENCY_PRESETS[0]!;
      if (existing && !existing.enabled) {
        const body = currencyPayload(
          {
            code: existing.code,
            exponent: String(existing.exponent),
            enabled: true,
            rate: String(Number(existing.rate) || 1),
            prefix: existing.prefix || preset.prefix,
            suffix: existing.suffix || preset.suffix,
            decimal_sep: existing.decimal_sep ?? ".",
            thousands_sep: existing.thousands_sep ?? ",",
            auto_update: existing.auto_update ?? true,
          },
          false,
        );
        if (!body) throw new ApiError(422, "error.validation", "currency");
        return api(`/admin/currencies/${existing.code}`, { method: "PUT", body });
      }
      const body = currencyPayload(
        {
          code: preset.code,
          exponent: "2",
          enabled: true,
          rate: "1",
          prefix: preset.prefix,
          suffix: preset.suffix,
          decimal_sep: ".",
          thousands_sep: ",",
          auto_update: true,
        },
        true,
      );
      if (!body) throw new ApiError(422, "error.validation", "currency");
      return api("/admin/currencies", { method: "POST", body });
    },
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "currencies"] });
      await refreshWizard();
      toast.success(t("common.saved"));
      setPhase("mail");
    },
    onError: fail,
  });

  const saveMail = useMutation({
    mutationFn: () =>
      api("/admin/settings/smtp", {
        method: "PUT",
        body: {
          enabled: smtp.enabled,
          host: smtp.host.trim(),
          port: Number(smtp.port) || 587,
          security: smtp.security,
          username: smtp.username.trim(),
          password: smtp.password || null,
          clear_password: false,
          from_address: smtp.from_address.trim(),
          from_name: smtp.from_name.trim(),
        },
      }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "settings", "smtp"] });
      await refreshWizard();
      toast.success(t("common.saved"));
      setPhase("site");
    },
    onError: fail,
  });

  const saveSite = useMutation({
    mutationFn: async () => {
      const cur = siteQ.data ?? normalizeConfig({});
      const next: SiteConfig = {
        ...cur,
        site_name: siteName.trim() || company.trim() || cur.site_name,
        tagline: tagline.trim(),
        logo_url: siteLogo.trim() || cur.logo_url,
      };
      return normalizeConfig(await api("/admin/site", { method: "PUT", body: next }));
    },
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "site"] });
      await qc.invalidateQueries({ queryKey: ["public", "site"] });
      await refreshWizard();
      try {
        localStorage.setItem(OOB_SITE_DONE_KEY, "1");
      } catch {
        /* ignore */
      }
      toast.success(t("common.saved"));
      setPhase("next");
    },
    onError: fail,
  });

  const enableGatewayQuick = useMutation({
    mutationFn: async (g: Gateway) => {
      const form = gatewayFormFromConfig(g);
      const { config, bad } = buildConfig(g.settings, form);
      if (bad) {
        toast.error(t("gateways.bad_json", { field: tCode(`gateways.setting.${bad}`) }));
        throw new Error("__oob_silent__");
      }
      await api(`/admin/gateways/${encodeURIComponent(g.id)}`, {
        method: "PUT",
        body: { enabled: true, config, secret_refs: {} },
      });
    },
    onSuccess: async () => {
      await invalidateWizardAndRelated();
      toast.success(t("common.saved"));
    },
    onError: (e: unknown) => {
      if (e instanceof Error && e.message === "__oob_silent__") return;
      fail(e);
    },
  });

  const createStarterProduct = useMutation({
    mutationFn: async () => {
      const groups = catalogGroupsQ.data?.items ?? [];
      let groupId: string;
      if (groups.length > 0) {
        groupId = groups[0]!.id;
      } else {
        const stamp = Date.now().toString(36);
        const g = await api<{ id: string }>("/admin/catalog/groups", {
          method: "POST",
          body: {
            name: "Services",
            slug: `starter-group-${stamp}`,
            sort_order: 0,
            hidden: false,
          },
        });
        groupId = g.id;
      }
      const name = siteName.trim() || company.trim() || "Starter";
      const slug = `${slugify(name)}-${Date.now().toString(36)}`;
      const p = await api<{ id: string }>("/admin/catalog/products", {
        method: "POST",
        body: {
          group_id: groupId,
          name,
          slug,
          description: "",
          status: "active",
        },
      });
      const code =
        currencies.data?.items.find((c) => c.enabled)?.code ??
        publicCurrenciesQ.data?.items[0]?.code ??
        "EUR";
      await api(`/admin/catalog/products/${p.id}/prices`, {
        method: "PUT",
        body: {
          prices: [{ currency: code, cycle: "monthly", amount: "9.99", setup_fee: "0" }],
        },
      });
    },
    onSuccess: async () => {
      await invalidateWizardAndRelated();
      toast.success(t("products.created"));
    },
    onError: fail,
  });

  const dismiss = useMutation({
    mutationFn: () =>
      api<Wizard>("/admin/setup-wizard/dismissed", {
        method: "PUT",
        body: { dismissed: true },
      }),
    onSuccess: (d) => {
      qc.setQueryData(WIZARD_KEY, d);
      setOpen(false);
    },
  });

  const w = wizard.data;
  const [done, total] = w ? requiredProgress(w) : [0, 0];
  const licenseDone = !!w?.steps.find((s) => s.id === "license")?.done;
  const remaining = useMemo(() => (w?.steps ?? []).filter((s) => !s.done), [w]);
  const requiredLeft = remaining.filter((s) => !s.optional);
  const optionalLeft = remaining.filter((s) => s.optional);
  const gatewayStepDone = !!w?.steps.find((s) => s.id === "gateway")?.done;
  const productStepDone = !!w?.steps.find((s) => s.id === "product")?.done;

  const gateways = gatewaysQ.data?.items ?? [];
  const manualGw = gateways.find((g) => g.kind === "manual");
  const gatewayToEnable = manualGw ?? gateways[0];
  const productsEmpty = (productsQ.data?.items.length ?? 0) === 0;
  const canQuickProduct =
    productsEmpty &&
    (catalogGroupsQ.data !== undefined || catalogGroupsQ.isSuccess) &&
    (currencies.data !== undefined || publicCurrenciesQ.data !== undefined);

  if (!w?.show) return null;

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) dismiss.mutate();
        else setOpen(true);
      }}
    >
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <RocketLaunchIcon className="size-5 text-primary" />
            {t("setup.oob_title")}
          </DialogTitle>
          <DialogDescription>{t("setup.oob_text", { done, total })}</DialogDescription>
        </DialogHeader>

        <StepRail phase={phase} />

        {phase === "license" && (
          <div className="grid gap-3">
            <p className="text-sm text-muted-foreground">{t("setup.step.license_hint")}</p>
            <p className="text-sm">{t("setup.oob.license_body")}</p>
            {licenseDone ? (
              <Button type="button" onClick={() => setPhase("company")}>
                {t("setup.oob_next")}
              </Button>
            ) : (
              <Button type="button" disabled={acceptLic.isPending || lic.isLoading} onClick={() => acceptLic.mutate()}>
                {t("license.accept")}
              </Button>
            )}
          </div>
        )}

        {phase === "company" && (
          <form
            className="grid gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              saveCompany.mutate();
            }}
          >
            <p className="flex items-center gap-2 text-sm font-medium">
              <BuildingsIcon className="size-4" />
              {t("setup.oob.phase.company")}
            </p>
            <div className="grid gap-3 sm:grid-cols-2">
              <div className="grid gap-2 sm:col-span-2">
                <Label htmlFor="oob-company">{t("general.f.general.company_name")}</Label>
                <Input
                  id="oob-company"
                  required
                  value={company}
                  onChange={(e) => setCompany(e.target.value)}
                  autoComplete="organization"
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="oob-email">{t("general.f.general.email")}</Label>
                <Input id="oob-email" type="email" required value={email} onChange={(e) => setEmail(e.target.value)} autoComplete="email" />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="oob-domain">{t("general.f.general.domain")}</Label>
                <Input
                  id="oob-domain"
                  type="url"
                  placeholder="https://example.com"
                  value={domain}
                  onChange={(e) => setDomain(e.target.value)}
                />
              </div>
              <div className="grid gap-2 sm:col-span-2">
                <Label htmlFor="oob-system">{t("general.f.system_urls.system_url")}</Label>
                <Input
                  id="oob-system"
                  type="url"
                  placeholder="https://billing.example.com"
                  value={systemUrl}
                  onChange={(e) => setSystemUrl(e.target.value)}
                />
                <p className="text-xs text-muted-foreground">{t("setup.oob.system_url_hint")}</p>
              </div>
              <ImageUrlField
                id="oob-logo"
                className="sm:col-span-2"
                label={t("general.f.general.logo_url")}
                value={companyLogo}
                onChange={setCompanyLogo}
                preferPath
                placeholder="/brand/logo.png"
                hint={t("setup.oob.logo_hint")}
              />
            </div>
            <div className="flex flex-wrap gap-2">
              <Button type="button" variant="outline" onClick={() => setPhase("license")}>
                {t("setup.oob_back")}
              </Button>
              <Button type="submit" disabled={saveCompany.isPending || !company.trim() || !email.trim()}>
                {t("setup.oob_save_continue")}
              </Button>
            </div>
          </form>
        )}

        {phase === "currency" && (
          <div className="grid gap-3">
            <p className="flex items-center gap-2 text-sm font-medium">
              <CurrencyCircleDollarIcon className="size-4" />
              {t("setup.oob.phase.currency")}
            </p>
            <p className="text-sm text-muted-foreground">{t("setup.oob.currency_hint")}</p>
            <div className="grid gap-2 sm:grid-cols-2">
              {CURRENCY_PRESETS.map((p) => {
                const on = currencyCode === p.code;
                const already = currencies.data?.items.some((c) => c.code === p.code && c.enabled);
                return (
                  <button
                    key={p.code}
                    type="button"
                    onClick={() => setCurrencyCode(p.code)}
                    className={cn(
                      "flex flex-col items-start gap-1 rounded-md border px-3 py-3 text-left text-sm transition-colors",
                      on ? "border-primary bg-primary/5" : "hover:bg-muted/60",
                    )}
                  >
                    <span className="font-medium">{p.label}</span>
                    <span className="text-xs text-muted-foreground">
                      {p.prefix || p.suffix || p.code}
                      {already ? ` · ${t("setup.oob.currency_ready")}` : ""}
                    </span>
                  </button>
                );
              })}
            </div>
            <div className="flex flex-wrap gap-2">
              <Button type="button" variant="outline" onClick={() => setPhase("company")}>
                {t("setup.oob_back")}
              </Button>
              <Button type="button" disabled={saveCurrency.isPending} onClick={() => saveCurrency.mutate()}>
                {t("setup.oob_save_continue")}
              </Button>
            </div>
          </div>
        )}

        {phase === "mail" && (
          <form
            className="grid gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              saveMail.mutate();
            }}
          >
            <p className="flex items-center gap-2 text-sm font-medium">
              <EnvelopeSimpleIcon className="size-4" />
              {t("setup.oob.phase.mail")}
            </p>
            <p className="text-sm text-muted-foreground">{t("setup.oob.mail_hint")}</p>
            <Label className="flex items-center gap-2">
              <Switch checked={smtp.enabled} onCheckedChange={(v) => setSmtp({ ...smtp, enabled: v })} />
              {t("smtp.enabled")}
            </Label>
            <div className="grid gap-3 sm:grid-cols-2">
              <div className="grid gap-2 sm:col-span-2">
                <Label htmlFor="oob-smtp-host">{t("smtp.host")}</Label>
                <Input
                  id="oob-smtp-host"
                  required={smtp.enabled}
                  value={smtp.host}
                  onChange={(e) => setSmtp({ ...smtp, host: e.target.value })}
                  placeholder="smtp.example.com"
                  autoComplete="off"
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="oob-smtp-port">{t("smtp.port")}</Label>
                <Input id="oob-smtp-port" inputMode="numeric" value={smtp.port} onChange={(e) => setSmtp({ ...smtp, port: e.target.value })} />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="oob-smtp-sec">{t("smtp.security")}</Label>
                <Select value={smtp.security} onValueChange={(v) => setSmtp({ ...smtp, security: v })}>
                  <SelectTrigger id="oob-smtp-sec" className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="starttls">{t("smtp.security_starttls")}</SelectItem>
                    <SelectItem value="tls">{t("smtp.security_tls")}</SelectItem>
                    <SelectItem value="none">{t("smtp.security_none")}</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div className="grid gap-2">
                <Label htmlFor="oob-smtp-user">{t("smtp.username")}</Label>
                <Input id="oob-smtp-user" value={smtp.username} onChange={(e) => setSmtp({ ...smtp, username: e.target.value })} autoComplete="off" />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="oob-smtp-pass">{t("smtp.password")}</Label>
                <Input
                  id="oob-smtp-pass"
                  type="password"
                  value={smtp.password}
                  onChange={(e) => setSmtp({ ...smtp, password: e.target.value })}
                  autoComplete="new-password"
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="oob-smtp-from">{t("smtp.from_address")}</Label>
                <Input
                  id="oob-smtp-from"
                  type="email"
                  required={smtp.enabled}
                  value={smtp.from_address}
                  onChange={(e) => setSmtp({ ...smtp, from_address: e.target.value })}
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="oob-smtp-from-name">{t("smtp.from_name")}</Label>
                <Input id="oob-smtp-from-name" value={smtp.from_name} onChange={(e) => setSmtp({ ...smtp, from_name: e.target.value })} />
              </div>
            </div>
            <div className="flex flex-wrap gap-2">
              <Button type="button" variant="outline" onClick={() => setPhase("currency")}>
                {t("setup.oob_back")}
              </Button>
              <Button type="button" variant="ghost" onClick={() => setPhase("site")}>
                {t("setup.oob_skip")}
              </Button>
              <Button type="submit" disabled={saveMail.isPending || (smtp.enabled && (!smtp.host.trim() || !smtp.from_address.trim()))}>
                {t("setup.oob_save_continue")}
              </Button>
            </div>
          </form>
        )}

        {phase === "site" && (
          <form
            className="grid gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              saveSite.mutate();
            }}
          >
            <p className="flex items-center gap-2 text-sm font-medium">
              <GlobeIcon className="size-4" />
              {t("setup.oob.phase.site")}
            </p>
            <p className="text-sm text-muted-foreground">{t("setup.oob.site_hint")}</p>
            <div className="grid gap-2">
              <Label htmlFor="oob-site-name">{t("site.admin.site_name")}</Label>
              <Input id="oob-site-name" required value={siteName} onChange={(e) => setSiteName(e.target.value)} placeholder={company || "PeltyCMS"} />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="oob-tagline">{t("site.admin.tagline")}</Label>
              <Input id="oob-tagline" value={tagline} onChange={(e) => setTagline(e.target.value)} placeholder={t("setup.oob.tagline_ph")} />
            </div>
            <ImageUrlField
              id="oob-site-logo"
              label={t("site.admin.logo_url")}
              value={siteLogo}
              onChange={setSiteLogo}
              preferPath
              placeholder="/brand/logo.png"
              hint={t("setup.oob.site_logo_hint")}
            />
            <div className="flex flex-wrap gap-2">
              <Button type="button" variant="outline" onClick={() => setPhase("mail")}>
                {t("setup.oob_back")}
              </Button>
              <Button type="submit" disabled={saveSite.isPending || !siteName.trim()}>
                {t("setup.oob_save_continue")}
              </Button>
            </div>
          </form>
        )}

        {phase === "next" && (
          <div className="grid gap-4">
            <p className="flex items-center gap-2 text-sm font-medium">
              <StorefrontIcon className="size-4" />
              {t("setup.oob.phase.next")}
            </p>
            <p className="text-sm text-muted-foreground">{t("setup.oob_next_steps")}</p>

            {(!gatewayStepDone || !productStepDone) && (
              <div className="grid gap-3 rounded-lg border bg-muted/30 p-3">
                {!gatewayStepDone && (
                  <div className="grid gap-2">
                    <p className="flex items-center gap-2 text-sm font-medium">
                      <CreditCardIcon className="size-4" />
                      {t("setup.oob.quick_gateway_title")}
                    </p>
                    <p className="text-xs text-muted-foreground">{t("setup.oob.quick_gateway_hint")}</p>
                    {gatewayToEnable ? (
                      <div className="flex flex-wrap items-center gap-2">
                        <Button
                          type="button"
                          size="sm"
                          disabled={enableGatewayQuick.isPending || gatewaysQ.isLoading || gatewayToEnable.enabled}
                          onClick={() => enableGatewayQuick.mutate(gatewayToEnable)}
                        >
                          {gatewayToEnable.kind === "manual"
                            ? t("setup.oob.enable_manual_gateway")
                            : t("setup.oob.enable_gateway", { name: gatewayToEnable.name })}
                        </Button>
                        <Button asChild variant="link" size="sm" className="h-auto px-0">
                          <Link to="/admin/gateways" onClick={() => setOpen(false)}>
                            {t("setup.oob.open_gateways")}
                          </Link>
                        </Button>
                      </div>
                    ) : (
                      <Button asChild variant="outline" size="sm" className="w-fit">
                        <Link to="/admin/gateways" onClick={() => setOpen(false)}>
                          {t("setup.oob.open_gateways")}
                        </Link>
                      </Button>
                    )}
                  </div>
                )}
                {!productStepDone && (
                  <div className="grid gap-2 border-t pt-3">
                    <p className="flex items-center gap-2 text-sm font-medium">
                      <PackageIcon className="size-4" />
                      {t("setup.oob.quick_product_title")}
                    </p>
                    <p className="text-xs text-muted-foreground">{t("setup.oob.quick_product_hint")}</p>
                    {productsEmpty ? (
                      <div className="flex flex-wrap items-center gap-2">
                        <Button type="button" size="sm" disabled={createStarterProduct.isPending || !canQuickProduct} onClick={() => createStarterProduct.mutate()}>
                          {t("setup.oob.create_starter_product")}
                        </Button>
                        <Button asChild variant="link" size="sm" className="h-auto px-0">
                          <Link to="/admin/products" onClick={() => setOpen(false)}>
                            {t("setup.oob.open_products")}
                          </Link>
                        </Button>
                      </div>
                    ) : (
                      <Button asChild variant="outline" size="sm" className="w-fit">
                        <Link to="/admin/products" onClick={() => setOpen(false)}>
                          {t("setup.oob.open_products")}
                        </Link>
                      </Button>
                    )}
                  </div>
                )}
              </div>
            )}

            {requiredLeft.length > 0 && (
              <div className="grid gap-2">
                <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">{t("setup.oob.required_left")}</p>
                {requiredLeft.map((s) => (
                  <Button key={s.id} asChild variant="outline" className="h-auto w-full justify-start gap-3 py-3">
                    <Link to={s.href} onClick={() => setOpen(false)}>
                      <CircleIcon className="size-5 shrink-0 text-muted-foreground" />
                      <span className="flex min-w-0 flex-1 flex-col items-start gap-0.5 text-left">
                        <span className="font-medium">{t(`setup.step.${s.id}` as TranslationKey)}</span>
                        <span className="text-xs font-normal text-muted-foreground">{t(`setup.step.${s.id}_hint` as TranslationKey)}</span>
                      </span>
                      <Badge variant="outline">{t("setup.configure")}</Badge>
                    </Link>
                  </Button>
                ))}
              </div>
            )}

            {optionalLeft.length > 0 && (
              <div className="grid gap-2">
                <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">{t("setup.optional")}</p>
                {optionalLeft.map((s) => (
                  <Button key={s.id} asChild variant="ghost" className="h-auto w-full justify-start gap-3 py-2">
                    <Link to={s.href} onClick={() => setOpen(false)}>
                      <CircleIcon className="size-4 shrink-0 text-muted-foreground" />
                      <span className="text-sm">{t(`setup.step.${s.id}` as TranslationKey)}</span>
                    </Link>
                  </Button>
                ))}
              </div>
            )}

            {remaining.length === 0 && (
              <p className="flex items-center gap-2 text-sm text-primary">
                <CheckCircleIcon className="size-5" weight="fill" />
                {t("setup.complete")}
              </p>
            )}

            <DialogFooter className="gap-2 sm:justify-between">
              <Button type="button" variant="ghost" onClick={() => dismiss.mutate()}>
                {t("setup.oob_later")}
              </Button>
              <Button asChild>
                <Link to="/admin/getting-started" onClick={() => setOpen(false)}>
                  {t("setup.open")}
                </Link>
              </Button>
            </DialogFooter>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
