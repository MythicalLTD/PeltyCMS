import { useState } from "react";
import { Link, useNavigate } from "react-router";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import * as z from "zod/mini";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Field, FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { Captcha } from "@/components/Captcha";
import { SsoButtons } from "@/components/account/SsoButtons";

const schema = z.object({
  email: z.email(),
  password: z.string().check(z.minLength(12), z.maxLength(128)),
  display_name: z.string().check(z.maxLength(100)),
});
type Values = z.infer<typeof schema>;

export function RegisterPage() {
  const { t, tCode } = useI18n();
  const navigate = useNavigate();
  const [error, setError] = useState<string | null>(null);
  const { register, handleSubmit, formState } = useForm<Values>({ resolver: zodResolver(schema), defaultValues: { display_name: "" } });

  const onSubmit = handleSubmit(async (v) => {
    setError(null);
    try {
      await api("/auth/register", { method: "POST", body: v });
      navigate("/login", { state: { registered: true } });
    } catch (e) {
      setError(e instanceof ApiError ? e.message || tCode(e.code) : t("common.error"));
    }
  });

  return (
    <Card>
      <CardHeader>
        <h1 className="text-2xl font-semibold tracking-tight">{t("auth.register.title")}</h1>
        <p className="text-sm text-muted-foreground">{t("auth.register.subtitle")}</p>
      </CardHeader>
      <CardContent>
        <form onSubmit={onSubmit} className="flex flex-col gap-4" noValidate>
          <FormError message={error} />
          <Field id="display_name" autoComplete="name" label={t("field.display_name")} placeholder={t("field.name_placeholder")} {...register("display_name")} />
          <Field id="email" type="email" autoComplete="email" label={t("field.email")} placeholder={t("field.email_placeholder")} error={formState.errors.email && t("validation.email")} {...register("email")} />
          <Field id="password" type="password" autoComplete="new-password" label={t("field.password")} placeholder="••••••••••••" hint={t("field.password_rules")} error={formState.errors.password && t("validation.password_min")} {...register("password")} />
          <Captcha form="register" />
          <Button type="submit" disabled={formState.isSubmitting}>{t("auth.register.submit")}</Button>
          <SsoButtons />
          <p className="text-center text-sm text-muted-foreground">
            {t("auth.register.have_account")}{" "}
            <Link to="/login" className="text-primary hover:underline">{t("auth.register.login_link")}</Link>
          </p>
        </form>
      </CardContent>
    </Card>
  );
}
