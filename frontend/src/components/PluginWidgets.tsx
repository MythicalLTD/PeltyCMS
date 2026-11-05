import { useQuery } from "@tanstack/react-query";
import { Card, CardContent } from "@/components/ui/card";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { usePluginSlot, type PluginUiEntry, type Slot } from "@/lib/addons/plugins";

function Widget({ entry }: { entry: PluginUiEntry }) {
  const { tCode } = useI18n();
  const { data, isError } = useQuery({
    queryKey: ["plugin-widget", entry.path],
    // Paths are validated server-side to live under /api/v1/plugins/<id>/.
    queryFn: () => api<{ title?: string; value: string }>(entry.path.replace(/^\/api\/v1/, "")),
  });
  return (
    <Card>
      <CardContent className="flex flex-col gap-1">
        <p className="text-xs text-muted-foreground">{tCode(data?.title ?? entry.label)}</p>
        <p className="font-mono text-2xl font-semibold tabular-nums">{isError ? "-" : (data?.value ?? "…")}</p>
      </CardContent>
    </Card>
  );
}

export function PluginWidgets({ slot }: { slot: Slot }) {
  const entries = usePluginSlot(slot);
  if (!entries.length) return null;
  return (
    <div className="grid gap-3 *:min-w-0 sm:grid-cols-2 lg:grid-cols-3">
      {entries.map((e) => <Widget key={e.plugin + e.path} entry={e} />)}
    </div>
  );
}
