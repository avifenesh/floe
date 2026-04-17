import { cn } from "@/lib/cn";

/** Tiny keyboard-glyph badge. Mono, hairline border, quiet muted tone. */
export function Kbd({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <kbd
      className={cn(
        "inline-flex items-center justify-center min-w-[18px] h-[18px] px-[5px]",
        "text-[11px] font-mono font-medium text-muted-foreground",
        "border rounded-[3px] bg-background",
        className,
      )}
    >
      {children}
    </kbd>
  );
}
