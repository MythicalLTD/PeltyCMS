import { Component, type ReactNode } from "react";
import { ArrowClockwiseIcon, WarningCircleIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { useI18n } from "@/lib/i18n";

function Fallback({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  const { t } = useI18n();
  return (
    <Card role="alert" className="mx-auto max-w-lg">
      <CardContent className="flex flex-col items-start gap-3">
        <div className="flex items-center gap-2 font-medium"><WarningCircleIcon className="size-5 text-destructive" />{t("common.page_error")}</div>
        <p className="break-words text-sm text-muted-foreground">{error instanceof Error ? error.message : String(error)}</p>
        <div className="flex flex-wrap gap-2">
          <Button onClick={() => window.location.reload()}><ArrowClockwiseIcon />{t("common.reload")}</Button>
          <Button variant="outline" onClick={onRetry}>{t("common.retry")}</Button>
        </div>
      </CardContent>
    </Card>
  );
}

/** Never leave a blank page: render errors show a message + reload. Reset via `resetKey` (e.g. pathname). */
export class PageErrorBoundary extends Component<{ children: ReactNode; resetKey?: string }, { error: unknown; key?: string }> {
  state: { error: unknown; key?: string } = { error: null, key: this.props.resetKey };
  static getDerivedStateFromError(error: unknown) { return { error: error ?? new Error("Unknown error") }; }
  static getDerivedStateFromProps(p: { resetKey?: string }, s: { error: unknown; key?: string }) {
    return p.resetKey !== s.key ? { error: null, key: p.resetKey } : null;
  }
  componentDidCatch(error: unknown) { console.error("Page render error", error); }
  render() {
    if (this.state.error) return <Fallback error={this.state.error} onRetry={() => this.setState({ error: null })} />;
    return this.props.children;
  }
}
