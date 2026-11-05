import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { CaretUpDownIcon, CheckIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { api, type Paged } from "@/lib/api";
import type { Customer } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export function customerLabel(c: Pick<Customer, "email" | "organization" | "first_name" | "last_name">): string {
  const name = c.organization?.trim() || `${c.first_name ?? ""} ${c.last_name ?? ""}`.trim();
  return name ? `${name} · ${c.email}` : c.email;
}

/** Async customer combobox: searches /admin/customers server-side (debounced). */
export function CustomerPicker({
  value,
  onChange,
  placeholder,
}: {
  value: string;
  onChange: (id: string, customer?: Customer) => void;
  placeholder?: string;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [dq, setDq] = useState("");
  const [picked, setPicked] = useState<Customer | null>(null);
  useEffect(() => { const h = setTimeout(() => setDq(q.trim()), 200); return () => clearTimeout(h); }, [q]);
  const [seenValue, setSeenValue] = useState(value);
  if (value !== seenValue) {
    setSeenValue(value);
    setPicked((p) => (!value ? null : p && p.id !== value ? null : p));
  }
  const { data, isFetching } = useQuery({
    queryKey: ["admin", "customers", "picker", dq],
    queryFn: () => api<Paged<Customer>>(`/admin/customers?per_page=25${dq ? `&q=${encodeURIComponent(dq)}` : ""}`),
    enabled: open,
  });
  const needResolve = !!value && (!picked || picked.id !== value);
  const resolve = useQuery({
    queryKey: ["admin", "customers", "picker", "one", value],
    queryFn: () => api<{ customer: Customer }>(`/admin/customers/${encodeURIComponent(value)}`),
    enabled: needResolve,
  });
  const [seenResolve, setSeenResolve] = useState(resolve.data);
  if (resolve.data !== seenResolve) {
    setSeenResolve(resolve.data);
    if (resolve.data?.customer) setPicked(resolve.data.customer);
  }

  const hint = placeholder ?? t("quotes.pick_customer");
  const shown = picked && picked.id === value ? customerLabel(picked) : value ? (resolve.isFetching ? t("common.loading") : value.slice(0, 8)) : hint;

  return (
    <Popover open={open} onOpenChange={(o) => { setOpen(o); if (o) setQ(""); }}>
      <PopoverTrigger asChild>
        <Button type="button" variant="outline" aria-expanded={open} aria-haspopup="listbox" className="h-10 w-full justify-between font-normal">
          <span className={cn("truncate", !value && "text-muted-foreground")}>{shown}</span>
          <CaretUpDownIcon className="opacity-50" aria-hidden />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-(--radix-popover-trigger-width) p-0" align="start">
        <Command shouldFilter={false}>
          <CommandInput value={q} onValueChange={setQ} placeholder={hint} />
          <CommandList>
            {isFetching && <div className="px-3 py-6 text-center text-sm text-muted-foreground">{t("common.loading")}</div>}
            {!isFetching && <CommandEmpty>{t("common.empty")}</CommandEmpty>}
            {!isFetching && (
              <CommandGroup>
                {(data?.items ?? []).map((c) => (
                  <CommandItem
                    key={c.id}
                    value={c.id}
                    onSelect={() => { onChange(c.id, c); setPicked(c); setOpen(false); }}
                    className="flex flex-col items-start gap-0.5 py-2"
                  >
                    <span className="flex w-full items-center gap-2">
                      <span className="min-w-0 flex-1 truncate font-medium">{c.organization?.trim() || `${c.first_name} ${c.last_name}`.trim() || c.email}</span>
                      <CheckIcon className={cn("size-4 shrink-0", value === c.id ? "opacity-100" : "opacity-0")} aria-hidden />
                    </span>
                    {(c.organization || c.first_name || c.last_name) && (
                      <span className="truncate text-xs text-muted-foreground">{c.email}</span>
                    )}
                  </CommandItem>
                ))}
              </CommandGroup>
            )}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
