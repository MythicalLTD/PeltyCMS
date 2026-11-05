import { useMemo, useState } from "react";
import { CaretUpDownIcon, CheckIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { useCountries, useStates, type GeoCountry, type GeoState } from "@/lib/geo";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** Fold diacritics so "caras" matches "Caraș-Severin". */
export function foldGeo(s: string) {
  return s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

function GeoCombobox({
  id,
  value,
  display,
  placeholder,
  searchPlaceholder,
  empty,
  disabled,
  items,
  onPick,
}: {
  id: string;
  value: string;
  display: string;
  placeholder: string;
  searchPlaceholder: string;
  empty: string;
  disabled?: boolean;
  items: { value: string; label: string; keywords: string }[];
  onPick: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const needle = foldGeo(q.trim());
  const filtered = useMemo(() => {
    if (!needle) return items;
    return items.filter((i) => foldGeo(i.keywords).includes(needle));
  }, [items, needle]);

  return (
    <Popover open={open} onOpenChange={(o) => { setOpen(o); if (o) setQ(""); }}>
      <PopoverTrigger asChild>
        <Button
          id={id}
          type="button"
          variant="outline"
          role="combobox"
          aria-expanded={open}
          disabled={disabled}
          className="h-9 w-full justify-between px-3 font-normal dark:bg-input/30"
        >
          <span className={cn("truncate", !value && "text-muted-foreground")}>{display || placeholder}</span>
          <CaretUpDownIcon className="size-4 shrink-0 opacity-50" aria-hidden />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-(--radix-popover-trigger-width) p-0" align="start">
        <Command shouldFilter={false}>
          <CommandInput value={q} onValueChange={setQ} placeholder={searchPlaceholder} />
          <CommandList>
            <CommandEmpty>{empty}</CommandEmpty>
            <CommandGroup>
              {filtered.map((i) => (
                <CommandItem
                  key={i.value}
                  value={i.value}
                  onSelect={() => { onPick(i.value); setOpen(false); }}
                >
                  <span className="min-w-0 flex-1 truncate">{i.label}</span>
                  <CheckIcon className={cn("size-4 shrink-0", value === i.value ? "opacity-100" : "opacity-0")} aria-hidden />
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

function countryItems(list: GeoCountry[]) {
  return list.map((c) => ({
    value: c.code,
    label: `${c.name} (${c.code})`,
    keywords: `${c.name} ${c.code}`,
  }));
}

function stateItems(list: GeoState[]) {
  return list.map((s) => ({
    value: s.name,
    label: s.name,
    keywords: `${s.name} ${s.code}`,
  }));
}

/** Country (ISO-2) + optional state/province searchable combobox. */
export function CountryRegionFields({
  country,
  region,
  onCountry,
  onRegion,
  countryId = "geo-country",
  regionId = "geo-region",
  required: _required,
  className,
  showRegion = true,
}: {
  country: string;
  region: string;
  onCountry: (code: string) => void;
  onRegion: (name: string) => void;
  countryId?: string;
  regionId?: string;
  required?: boolean;
  className?: string;
  showRegion?: boolean;
}) {
  const { t } = useI18n();
  const countries = useCountries();
  const code = country.trim().toUpperCase();
  const states = useStates(code);
  const hasStates = (states.data?.items.length ?? 0) > 0;
  const countryLabel = countries.data?.items.find((c) => c.code === code)?.name;
  const regionInList = hasStates && states.data!.items.some((s) => s.name === region || s.code === region);
  const regionDisplay = regionInList
    ? (states.data!.items.find((s) => s.name === region || s.code === region)?.name ?? region)
    : region;

  return (
    <div className={cn("grid gap-3 sm:grid-cols-2", className)}>
      <div className="grid gap-1.5">
        <Label htmlFor={countryId}>{t("billing.country")}</Label>
        <GeoCombobox
          id={countryId}
          value={code}
          display={countryLabel ? `${countryLabel} (${code})` : code}
          placeholder={t("geo.country_placeholder")}
          searchPlaceholder={t("geo.search_country")}
          empty={t("geo.no_matches")}
          items={countryItems(countries.data?.items ?? [])}
          onPick={(next) => {
            onCountry(next);
            if (next !== code) onRegion("");
          }}
        />
        {countries.isError && <p className="text-xs text-destructive">{t("geo.load_error")}</p>}
      </div>
      {showRegion && (
        <div className="grid gap-1.5">
          <Label htmlFor={regionId}>{t("profile.region")}</Label>
          {hasStates ? (
            <GeoCombobox
              id={regionId}
              value={regionDisplay}
              display={regionDisplay}
              placeholder={t("geo.region_placeholder")}
              searchPlaceholder={t("geo.search_region")}
              empty={t("geo.no_matches")}
              items={stateItems(states.data!.items)}
              onPick={onRegion}
            />
          ) : (
            <Input
              id={regionId}
              maxLength={100}
              value={region}
              autoComplete="address-level1"
              placeholder={code ? t("geo.region_free") : t("geo.region_pick_country")}
              disabled={!code}
              onChange={(e) => onRegion(e.target.value)}
            />
          )}
        </div>
      )}
    </div>
  );
}

/** Country-only searchable select. */
export function CountrySelect({
  id = "geo-country",
  value,
  onChange,
  required: _required,
  className,
  allowEmpty: _allowEmpty = true,
}: {
  id?: string;
  value: string;
  onChange: (code: string) => void;
  required?: boolean;
  className?: string;
  allowEmpty?: boolean;
}) {
  const { t } = useI18n();
  const countries = useCountries();
  const code = value.trim().toUpperCase();
  const label = countries.data?.items.find((c) => c.code === code)?.name;
  return (
    <div className={className}>
      <GeoCombobox
        id={id}
        value={code}
        display={label ? `${label} (${code})` : code}
        placeholder={t("geo.country_placeholder")}
        searchPlaceholder={t("geo.search_country")}
        empty={t("geo.no_matches")}
        items={countryItems(countries.data?.items ?? [])}
        onPick={onChange}
      />
    </div>
  );
}
