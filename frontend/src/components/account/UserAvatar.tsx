import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { cn } from "@/lib/utils";

/** Up to two initials from a display name or e-mail ("Ana Pop" -> "AP", "ana@x.ro" -> "AN"). */
export function initials(name?: string | null, email?: string | null): string {
  const src = (name || "").trim();
  if (src) {
    const parts = src.split(/\s+/);
    const first = parts[0] ?? "";
    const last = parts.length > 1 ? (parts[parts.length - 1] ?? "") : "";
    return (last ? first.slice(0, 1) + last.slice(0, 1) : first.slice(0, 2)).toUpperCase();
  }
  return ((email || "?").split("@")[0] ?? "?").slice(0, 2).toUpperCase();
}

/** Profile picture with an initials fallback. Only requests the image when the user has one (no Gravatar). */
export function UserAvatar({ userId, version, name, email, size = "default", className }: {
  userId?: string | null; version?: number | null; name?: string | null; email?: string | null;
  size?: "sm" | "default" | "lg"; className?: string;
}) {
  const px = size === "lg" ? 256 : 64;
  return (
    <Avatar size={size} className={className}>
      {userId && version != null && <AvatarImage src={`/api/v1/avatars/${userId}?s=${px}&v=${version}`} alt="" />}
      <AvatarFallback className={cn("bg-primary/15 font-medium text-primary")}>{initials(name, email)}</AvatarFallback>
    </Avatar>
  );
}
