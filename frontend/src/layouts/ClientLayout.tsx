import { ShieldCheckIcon } from "@phosphor-icons/react";
import { Shell } from "./Shell";
import { isStaff, useMe } from "@/lib/auth";
import { GeneralRuntime } from "@/components/admin/GeneralRuntime";
import { ChatWidget } from "@/components/support/ChatWidget";
import { flatNav } from "@/lib/navRegistry";

/** Flat view of the client registry (kept for tests and older imports). */
export const CLIENT_NAV = flatNav("client");

export function ClientLayout() {
  const { data: me } = useMe();
  return (
    <>
      <GeneralRuntime banner={false} />
      <Shell slot="client.nav" scope="client" section="nav.client_area" footer={isStaff(me) ? { to: "/admin", label: "nav.admin", icon: ShieldCheckIcon } : undefined} />
      <ChatWidget />
    </>
  );
}
