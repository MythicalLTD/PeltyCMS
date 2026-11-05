import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export type Tone = "neutral" | "success" | "warning" | "danger" | "primary";
/** Semantic tone -> Badge status-pill variant (soft tinted colours from theme tokens). */
export function toneVariant(t: Tone | string): "success" | "warning" | "danger" | "info" | "muted" {
  return t === "danger" ? "danger" : t === "success" ? "success" : t === "primary" ? "info" : t === "warning" ? "warning" : "muted";
}
