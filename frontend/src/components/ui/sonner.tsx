import {
  CheckCircleIcon as CircleCheckIcon,
  InfoIcon,
  SpinnerGapIcon as Loader2Icon,
  XCircleIcon as OctagonXIcon,
  WarningIcon as TriangleAlertIcon,
} from "@phosphor-icons/react"
import { useEffect, useState } from "react"
import { Toaster as Sonner, type ToasterProps } from "sonner"

const Toaster = ({ ...props }: ToasterProps) => {
  // PeltyCMS themes toggle the `dark` class on <html> (lib/theme.ts).
  // Track it live: the active theme loads after first render and can change.
  const isDark = () => document.documentElement.classList.contains("dark")
  const [theme, setTheme] = useState(isDark() ? "dark" : "light")
  useEffect(() => {
    const o = new MutationObserver(() => setTheme(isDark() ? "dark" : "light"))
    o.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] })
    return () => o.disconnect()
  }, [])

  return (
    <Sonner
      theme={theme as ToasterProps["theme"]}
      className="toaster group"
      toastOptions={{
        classNames: {
          toast: "shadow-md border-border",
          success: "[&_[data-icon]]:text-success",
          error: "[&_[data-icon]]:text-destructive",
          warning: "[&_[data-icon]]:text-warning",
          info: "[&_[data-icon]]:text-primary",
        },
      }}
      icons={{
        success: <CircleCheckIcon className="size-4" />,
        info: <InfoIcon className="size-4" />,
        warning: <TriangleAlertIcon className="size-4" />,
        error: <OctagonXIcon className="size-4" />,
        loading: <Loader2Icon className="size-4 animate-spin" />,
      }}
      style={
        {
          "--normal-bg": "var(--popover)",
          "--normal-text": "var(--popover-foreground)",
          "--normal-border": "var(--border)",
          "--border-radius": "var(--radius)",
        } as React.CSSProperties
      }
      {...props}
    />
  )
}

export { Toaster }
