/** Three-dot loading indicator. Stays in baseline-flow with
 *  surrounding text — drops a tiny animated strip that pulses
 *  without shifting layout. Use inline next to loading copy
 *  ("Intent & Proof", "analysing"). */
export function LoadingDots({
  className = "",
}: {
  className?: string;
}) {
  return (
    <span
      aria-hidden
      className={"inline-flex items-baseline gap-[3px] " + className}
    >
      <Dot delay="0s" />
      <Dot delay="0.2s" />
      <Dot delay="0.4s" />
    </span>
  );
}

function Dot({ delay }: { delay: string }) {
  return (
    <span
      className="inline-block w-1 h-1 rounded-full bg-muted-foreground/70"
      style={{
        animation: "floeLoadingDot 1.2s ease-in-out infinite",
        animationDelay: delay,
      }}
    />
  );
}
