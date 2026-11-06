import { useEffect } from "react";
import { CustomCodeOff } from "@/components/admin/GeneralRuntime";
import { LicenseAcceptGate } from "@/components/admin/LicenseGate";
import { SquaresFourIcon } from "@phosphor-icons/react";
import { setThemeChromeEnabled } from "@/lib/theme";
import { Shell } from "./Shell";
import { ReauthDialog } from "@/components/account/ReauthDialog";

/** Admin never loads client-area theme stylesheets / custom CSS. */
function ThemeChromeOff() {
  useEffect(() => {
    setThemeChromeEnabled(false);
    return () => setThemeChromeEnabled(true);
  }, []);
  return null;
}

export function AdminLayout() {
  return (
    <>
      <CustomCodeOff />
      <ThemeChromeOff />
      <ReauthDialog />
      <LicenseAcceptGate />
      <Shell slot="admin.nav" scope="admin" section="nav.admin" footer={{ to: "/", label: "nav.client_area", icon: SquaresFourIcon }} />
    </>
  );
}
