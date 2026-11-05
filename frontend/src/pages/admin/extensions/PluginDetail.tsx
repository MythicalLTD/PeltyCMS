import { Switch } from "@/components/ui/switch";
import { PageHeader } from "@/components/PageHeader";
import { useState } from "react";
import { Link, useParams } from "react-router";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { SettingsSchema } from "@/lib/addons/plugins";

type Values = Record<string, string | number | boolean>;

/** Generic plugin page: settings form rendered from the manifest's settings_schema. */
export function AdminPluginDetailPage() {
  const { id = "" } = useParams();
  const { t, tCode } = useI18n();
  const { data, refetch } = useQuery({
    queryKey: ["admin", "plugin-settings", id],
    queryFn: () => api<{ schema: SettingsSchema | null; values: Values }>(`/admin/plugins/${encodeURIComponent(id)}/settings`),
  });
  // External plugins with a webhook_url get a delivery log page (404 otherwise).
  const hook = useQuery({ queryKey: ["admin", "plugin-webhook", id], queryFn: () => api<unknown>(`/admin/plugins/${encodeURIComponent(id)}/webhook`), retry: false });
  const [values, setValues] = useState<Values>(data?.values ?? {});
  const [seen, setSeen] = useState(data);
  if (data !== seen) {
    setSeen(data);
    if (data !== undefined) setValues(data.values ?? {});
  }
  const save = useMutation({
    mutationFn: () => api<void>(`/admin/plugins/${encodeURIComponent(id)}/settings`, { method: "PUT", body: values }),
    onSuccess: () => refetch(),
  });
  const props = Object.entries(data?.schema?.properties ?? {});
  return (
    <div className="flex flex-col gap-4">
      <Button asChild variant="ghost" size="sm" className="self-start"><Link to="/admin/plugins">← {t("nav.plugins")}</Link></Button>
      <PageHeader title={tCode(`plugin.${id}.title`) === `plugin.${id}.title` ? id : tCode(`plugin.${id}.title`)}
        actions={hook.isSuccess && <Button asChild variant="outline"><Link to={`/admin/plugins/${encodeURIComponent(id)}/deliveries`}>{t("plugins.webhook.deliveries")}</Link></Button>} />
      <Card>
        <CardHeader><CardTitle>{t("admin.plugins.settings")}</CardTitle></CardHeader>
        <CardContent>
          {props.length === 0 ? (
            <p className="text-sm text-muted-foreground">{t("admin.plugins.no_settings")}</p>
          ) : (
            <form
              className="flex max-w-md flex-col gap-4"
              onSubmit={(e) => {
                e.preventDefault();
                save.mutate();
              }}
            >
              {props.map(([name, p]) => {
                const label = p.title ? tCode(p.title) : name;
                const v = values[name];
                if (p.type === "boolean") {
                  return (
                    <div key={name} className="flex min-h-11 items-center gap-3">
                      <Switch id={`f-${name}`} checked={v === true} onCheckedChange={(c) => setValues({ ...values, [name]: c })} />
                      <Label htmlFor={`f-${name}`}>{label}</Label>
                    </div>
                  );
                }
                const numeric = p.type === "number" || p.type === "integer";
                return (
                  <div key={name} className="flex flex-col gap-1.5">
                    <Label htmlFor={`s-${name}`}>{label}</Label>
                    <Input
                      id={`s-${name}`}
                      type={numeric ? "number" : "text"}
                      step={p.type === "integer" ? 1 : "any"}
                      value={v === undefined ? "" : String(v)}
                      onChange={(e) => {
                        const next = { ...values };
                        if (e.target.value === "") delete next[name];
                        else next[name] = numeric ? Number(e.target.value) : e.target.value;
                        setValues(next);
                      }}
                    />
                  </div>
                );
              })}
              {save.error instanceof ApiError && <p role="alert" className="text-sm text-destructive">{save.error.message}</p>}
              {save.isSuccess && <output className="block text-sm text-success">{t("common.saved")}</output>}
              <Button type="submit" disabled={save.isPending} className="self-start">{t("common.save")}</Button>
            </form>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
