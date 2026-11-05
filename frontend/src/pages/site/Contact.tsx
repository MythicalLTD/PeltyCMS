import { useState } from "react";
import { CheckCircleIcon, EnvelopeIcon } from "@phosphor-icons/react";
import { useMutation } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Captcha } from "@/components/Captcha";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { validEmail } from "@/lib/support/supportExt";

/** Public contact form (pre-sales); messages land in the admin contact inbox. */
export function ContactPage() {
  const { t, tCode } = useI18n();
  const [f, setF] = useState({ name: "", email: "", subject: "", message: "", website: "" });
  const [error, setError] = useState<string | null>(null);
  const send = useMutation({
    mutationFn: () => api("/public/contact", { method: "POST", body: f }),
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const set = (k: keyof typeof f) => (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => setF((x) => ({ ...x, [k]: e.target.value }));
  const ok = f.name.trim() && validEmail(f.email) && f.message.trim();
  return (
    <div className="mx-auto w-full max-w-xl p-4 sm:p-6">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2"><EnvelopeIcon aria-hidden />{t("contact.title")}</CardTitle>
          <CardDescription>{t("contact.subtitle")}</CardDescription>
        </CardHeader>
        <CardContent>
          {send.isSuccess ? (
            <p className="flex items-center gap-2 text-sm"><CheckCircleIcon aria-hidden className="text-primary" />{t("contact.sent")}</p>
          ) : (
            <form className="grid gap-3" onSubmit={(e) => { e.preventDefault(); setError(null); send.mutate(); }}>
              <div className="grid gap-1.5"><Label htmlFor="c-name">{t("contact.name")}</Label><Input id="c-name" autoComplete="name" maxLength={100} value={f.name} onChange={set("name")} required /></div>
              <div className="grid gap-1.5"><Label htmlFor="c-email">{t("contact.email")}</Label><Input id="c-email" type="email" autoComplete="email" value={f.email} onChange={set("email")} required /></div>
              <div className="grid gap-1.5"><Label htmlFor="c-subject">{t("contact.subject")}</Label><Input id="c-subject" maxLength={200} value={f.subject} onChange={set("subject")} /></div>
              <div className="grid gap-1.5"><Label htmlFor="c-msg">{t("contact.message")}</Label><Textarea id="c-msg" rows={6} maxLength={10000} value={f.message} onChange={set("message")} required /></div>
              {/* Honeypot: hidden from people and assistive tech, bots fill it. */}
              <input type="text" name="website" tabIndex={-1} autoComplete="off" aria-hidden="true" className="hidden" value={f.website} onChange={set("website")} />
              <Captcha form="contact" />
              <FormError message={error} />
              <Button type="submit" disabled={!ok || send.isPending}>{t("contact.send")}</Button>
            </form>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
