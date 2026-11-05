import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { CheckCircleIcon, FlaskIcon, UploadSimpleIcon, WarningIcon } from "@phosphor-icons/react";
import { PageHeader } from "@/components/PageHeader";
import { FormError } from "@/components/Field";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { api, ApiError } from "@/lib/api";
import { cleanMapping, IMPORT_KINDS, missingRequired, type Analysis, type ImportKind, type ImportReport } from "@/lib/admin/csvImport";
import { useI18n } from "@/lib/i18n";

const NONE = "__none";

export function AdminCsvImportPage() {
  const { t, tCode } = useI18n();
  const [kind, setKind] = useState<ImportKind>("clients");
  const [csv, setCsv] = useState("");
  const [fileName, setFileName] = useState("");
  const [an, setAn] = useState<Analysis | null>(null);
  const [mapping, setMapping] = useState<Record<string, string>>({});
  const [report, setReport] = useState<ImportReport | null>(null);
  const err = (e: unknown) => (e ? (e instanceof ApiError ? tCode(e.code) : t("error.internal")) : null);
  const analyze = useMutation({
    mutationFn: (text: string) => api<Analysis>("/admin/import/csv/analyze", { method: "POST", body: { kind, csv: text } }),
    onSuccess: (a) => { setAn(a); setMapping(a.mapping); setReport(null); },
  });
  const run = useMutation({
    mutationFn: (dry: boolean) => api<ImportReport>("/admin/import/csv/run", { method: "POST", body: { kind, csv, mapping: cleanMapping(mapping), dry_run: dry } }),
    onSuccess: setReport,
  });
  const reset = () => { setAn(null); setReport(null); setCsv(""); setFileName(""); analyze.reset(); run.reset(); };
  const onFile = async (f: File | undefined) => {
    if (!f) return;
    const text = await f.text();
    setCsv(text); setFileName(f.name); analyze.mutate(text);
  };
  const missing = an ? missingRequired(an.fields, mapping) : [];
  return (
    <div className="flex w-full flex-col gap-6">
      <PageHeader title={t("csv.title")} description={t("csv.subtitle")} />
      <Card>
        <CardHeader><CardTitle>{t("csv.step_file")}</CardTitle><CardDescription>{t("csv.file_hint")}</CardDescription></CardHeader>
        <CardContent className="grid gap-4 sm:grid-cols-2">
          <div className="grid gap-1.5">
            <Label>{t("csv.kind")}</Label>
            <Select value={kind} onValueChange={(v) => { setKind(v as ImportKind); reset(); }}>
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>{IMPORT_KINDS.map((k) => <SelectItem key={k} value={k}>{t(`csv.kind.${k}`)}</SelectItem>)}</SelectContent>
            </Select>
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="csv-file">{t("csv.file")}</Label>
            <Input key={kind} id="csv-file" type="file" accept=".csv,text/csv" onChange={(e) => void onFile(e.target.files?.[0])} />
            {fileName && an && <p className="text-xs text-muted-foreground">{t("csv.rows", { n: an.rows })}</p>}
          </div>
          <div className="sm:col-span-2"><FormError message={err(analyze.error)} /></div>
        </CardContent>
      </Card>
      {an && (
        <Card>
          <CardHeader><CardTitle>{t("csv.step_map")}</CardTitle><CardDescription>{t("csv.map_hint")}</CardDescription></CardHeader>
          <CardContent className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {an.fields.map((f) => (
              <div key={f.key} className="grid gap-1.5">
                <Label>{tCode(`csv.field.${f.key}`)}{f.required && " *"}</Label>
                <Select value={mapping[f.key] || NONE} onValueChange={(v) => { setMapping((m) => ({ ...m, [f.key]: v === NONE ? "" : v })); setReport(null); }}>
                  <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value={NONE}>{t("csv.skip_column")}</SelectItem>
                    {an.headers.filter((h) => h !== "").map((h) => <SelectItem key={h} value={h}>{h}</SelectItem>)}
                  </SelectContent>
                </Select>
              </div>
            ))}
          </CardContent>
          <CardContent>
            <p className="mb-2 text-sm font-medium">{t("csv.preview")}</p>
            <div className="overflow-x-auto rounded-md border">
              <Table>
                <TableHeader><TableRow>{an.headers.map((h, i) => <TableHead key={i}>{h}</TableHead>)}</TableRow></TableHeader>
                <TableBody>{an.preview.map((r, i) => <TableRow key={i}>{an.headers.map((_, j) => <TableCell key={j} className="max-w-48 truncate">{r[j] ?? ""}</TableCell>)}</TableRow>)}</TableBody>
              </Table>
            </div>
          </CardContent>
          <CardFooter className="flex flex-wrap gap-2">
            {missing.length > 0 && <p className="w-full text-sm text-destructive">{t("csv.missing", { fields: missing.map((k) => tCode(`csv.field.${k}`)).join(", ") })}</p>}
            <Button variant="outline" disabled={missing.length > 0 || run.isPending} onClick={() => run.mutate(true)}><FlaskIcon aria-hidden />{t("csv.dry_run")}</Button>
            <Button disabled={missing.length > 0 || run.isPending || !report || report.committed || report.error_count > 0} onClick={() => run.mutate(false)}><UploadSimpleIcon aria-hidden />{t("csv.import")}</Button>
            <FormError message={err(run.error)} />
          </CardFooter>
        </Card>
      )}
      {report && (
        <Card>
          <CardHeader><CardTitle>{report.committed ? t("csv.done") : t("csv.dry_result")}</CardTitle></CardHeader>
          <CardContent className="grid gap-4">
            <div className="flex flex-wrap gap-2">
              <Badge variant="secondary">{t("csv.total", { n: report.total })}</Badge>
              <Badge variant="secondary">{t(report.committed ? "csv.created" : "csv.would_create", { n: report.created })}</Badge>
              <Badge variant="outline">{t("csv.skipped", { n: report.skipped })}</Badge>
              {report.error_count > 0 && <Badge variant="destructive">{t("csv.errors", { n: report.error_count })}</Badge>}
            </div>
            {report.error_count === 0 ? (
              <Alert><CheckCircleIcon aria-hidden /><AlertTitle>{report.committed ? t("csv.done") : t("csv.ready")}</AlertTitle><AlertDescription>{report.committed ? t("csv.done_hint") : t("csv.ready_hint")}</AlertDescription></Alert>
            ) : (
              <Alert variant="destructive"><WarningIcon aria-hidden /><AlertTitle>{t("csv.fix_errors")}</AlertTitle>
                <AlertDescription>
                  <ul className="grid gap-1">{report.errors.map((e, i) => <li key={i}>{t("csv.row", { n: e.row })}{e.field && ` · ${tCode(`csv.field.${e.field}`)}`}: {tCode(e.code)}</li>)}</ul>
                </AlertDescription>
              </Alert>
            )}
          </CardContent>
        </Card>
      )}
    </div>
  );
}
