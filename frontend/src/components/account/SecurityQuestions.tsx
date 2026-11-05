import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircleIcon, TrashIcon, XCircleIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { ConfirmAction } from "@/components/ConfirmAction";
import { FormError } from "@/components/Field";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

interface Question { id: string; question: string }
interface Current { question_id: string; question: string; updated_at: string }

/** Client account: choose a security question and store a (hashed) answer. */
export function SecurityQuestionCard() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["account", "security-question"], queryFn: () => api<{ questions: Question[]; current: Current | null }>("/account/security-question") });
  const [qid, setQid] = useState("");
  const [answer, setAnswer] = useState("");
  const [password, setPassword] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const done = () => void qc.invalidateQueries({ queryKey: ["account", "security-question"] });
  const fail = (e: unknown) => setErr(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const save = useMutation({
    mutationFn: () => api("/account/security-question", { method: "PUT", body: { question_id: qid || q.data?.current?.question_id, answer, password } }),
    onSuccess: () => { setErr(null); setAnswer(""); setPassword(""); done(); toast.success(t("common.saved")); },
    onError: fail,
  });
  const clear = useMutation({ mutationFn: () => api("/account/security-question", { method: "DELETE" }), onSuccess: done, onError: fail });
  if (!q.data || q.data.questions.length === 0) return null;
  const cur = q.data.current;
  const selected = qid || cur?.question_id || "";
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("secq.title")}</CardTitle>
        <CardDescription>{cur ? t("secq.current", { question: cur.question, date: formatDateTime(cur.updated_at, lang) }) : t("secq.hint")}</CardDescription>
      </CardHeader>
      <CardContent>
        <form className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <div className="flex flex-col gap-2">
            <Label htmlFor="secq-q">{t("secq.question")}</Label>
            <Select value={selected} onValueChange={setQid}>
              <SelectTrigger id="secq-q" className="w-full"><SelectValue placeholder={t("secq.choose")} /></SelectTrigger>
              <SelectContent>{q.data.questions.map((x) => <SelectItem key={x.id} value={x.id}>{x.question}</SelectItem>)}</SelectContent>
            </Select>
          </div>
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="flex flex-col gap-2"><Label htmlFor="secq-a">{t("secq.answer")}</Label><Input id="secq-a" autoComplete="off" value={answer} maxLength={200} onChange={(e) => setAnswer(e.target.value)} /></div>
            <div className="flex flex-col gap-2"><Label htmlFor="secq-p">{t("secq.password")}</Label><Input id="secq-p" type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} /></div>
          </div>
          <FormError message={err} />
          <div className="flex flex-wrap justify-end gap-2">
            {cur && <ConfirmAction description={t("secq.clear_confirm")} onConfirm={() => clear.mutate()}><Button type="button" variant="outline" className="min-h-11 sm:min-h-9">{t("secq.clear")}</Button></ConfirmAction>}
            <Button type="submit" className="min-h-11 sm:min-h-9" disabled={!selected || !answer.trim() || !password || save.isPending}>{t("common.save")}</Button>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}

/** Admin settings: the list of questions clients can pick from. */
export function SecurityQuestionsAdminCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "security-questions"], queryFn: () => api<{ items: Question[] }>("/admin/security-questions") });
  const [text, setText] = useState("");
  const done = () => void qc.invalidateQueries({ queryKey: ["admin", "security-questions"] });
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const add = useMutation({ mutationFn: () => api("/admin/security-questions", { method: "POST", body: { question: text } }), onSuccess: () => { setText(""); done(); }, onError: fail });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/security-questions/${id}`, { method: "DELETE" }), onSuccess: done, onError: fail });
  return (
    <Card>
      <CardHeader><CardTitle>{t("secq.admin_title")}</CardTitle><CardDescription>{t("secq.admin_hint")}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-3">
        <form className="flex flex-col gap-2 sm:flex-row" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
          <Input aria-label={t("secq.question")} placeholder={t("secq.new_placeholder")} value={text} maxLength={200} onChange={(e) => setText(e.target.value)} />
          <Button type="submit" className="min-h-11 sm:min-h-9" disabled={!text.trim() || add.isPending}>{t("common.add")}</Button>
        </form>
        <ul className="flex flex-col divide-y rounded-md border">
          {(q.data?.items ?? []).length === 0 && <li className="p-3 text-sm text-muted-foreground">{t("secq.none")}</li>}
          {q.data?.items.map((x) => (
            <li key={x.id} className="flex items-center justify-between gap-2 p-3 text-sm">
              <span className="break-words">{x.question}</span>
              <ConfirmAction description={t("secq.delete_confirm")} onConfirm={() => del.mutate(x.id)}>
                <Button size="sm" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button>
              </ConfirmAction>
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}

/** Admin client page: check a caller's answer without seeing it. */
export function VerifySecurityAnswerCard({ customerId }: { customerId: string }) {
  const { t, tCode } = useI18n();
  const q = useQuery({ queryKey: ["admin", "customer", customerId, "secq"], queryFn: () => api<{ current: Current | null }>(`/admin/customers/${customerId}/security-question`) });
  const [answer, setAnswer] = useState("");
  const [result, setResult] = useState<boolean | null>(null);
  const check = useMutation({
    mutationFn: () => api<{ match: boolean }>(`/admin/customers/${customerId}/security-question/verify`, { method: "POST", body: { answer } }),
    onSuccess: (d) => setResult(d.match),
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (!q.data) return null;
  return (
    <Card>
      <CardHeader><CardTitle>{t("secq.verify_title")}</CardTitle><CardDescription>{q.data.current ? q.data.current.question : t("secq.not_set")}</CardDescription></CardHeader>
      {q.data.current && (
        <CardContent>
          <form className="flex flex-col gap-2 sm:flex-row sm:items-center" onSubmit={(e) => { e.preventDefault(); check.mutate(); }}>
            <Input aria-label={t("secq.answer")} autoComplete="off" value={answer} onChange={(e) => { setAnswer(e.target.value); setResult(null); }} />
            <Button type="submit" className="min-h-11 sm:min-h-9" disabled={!answer.trim() || check.isPending}>{t("secq.verify")}</Button>
            {result !== null && (result
              ? <Badge variant="secondary"><CheckCircleIcon aria-hidden />{t("secq.match")}</Badge>
              : <Badge variant="destructive"><XCircleIcon aria-hidden />{t("secq.no_match")}</Badge>)}
          </form>
        </CardContent>
      )}
    </Card>
  );
}
