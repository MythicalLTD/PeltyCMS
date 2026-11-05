import * as React from "react"
import { cn } from "@/lib/utils"

/** Native `<select>` styled like `Input` so dark theme fields stay consistent. */
function NativeSelect({ className, value, defaultValue, ...props }: React.ComponentProps<"select">) {
  const empty = value === undefined ? defaultValue === "" || defaultValue === undefined : value === ""
  return (
    <select
      data-slot="native-select"
      value={value}
      defaultValue={defaultValue}
      className={cn(
        "pointer-coarse:min-h-11 h-9 w-full min-w-0 cursor-pointer rounded-md border border-input bg-transparent px-3 py-1 text-base shadow-xs transition-[color,box-shadow] outline-none disabled:cursor-not-allowed disabled:opacity-50 md:text-sm dark:bg-input/30",
        empty ? "text-muted-foreground" : "text-foreground",
        "focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50",
        "aria-invalid:border-destructive aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40",
        "[&>option]:bg-popover [&>option]:text-popover-foreground",
        className
      )}
      {...props}
    />
  )
}

export { NativeSelect }
