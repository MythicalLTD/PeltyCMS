import { CustomCodeOff } from "@/components/admin/GeneralRuntime";
import { LicenseAcceptGate } from "@/components/admin/LicenseGate";
import { SquaresFourIcon } from "@phosphor-icons/react";
import { Shell } from "./Shell";
import { ReauthDialog } from "@/components/account/ReauthDialog";

export function AdminLayout() {
  return (
    <>
      <CustomCodeOff />
      <ReauthDialog />
      <LicenseAcceptGate />
      <Shell slot="admin.nav" scope="admin" section="nav.admin" footer={{ to: "/", label: "nav.client_area", icon: SquaresFourIcon }} />
    </>
  );
}
