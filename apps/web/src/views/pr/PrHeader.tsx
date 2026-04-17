import type { Artifact } from "@/types/artifact";
import { deriveSlug, isPathSha, shortSha } from "@/lib/artifact";

export function PrHeader({ artifact }: { artifact: Artifact }) {
  const slug = deriveSlug(artifact.pr.base_sha, artifact.pr.head_sha);
  const showSha = !isPathSha(artifact.pr.base_sha) && !isPathSha(artifact.pr.head_sha);
  return (
    <header className="space-y-1.5">
      <h1 className="text-[15px] font-mono font-semibold text-foreground">{slug}</h1>
      {showSha && (
        <div className="text-[11px] font-mono text-muted-foreground flex items-center gap-2">
          <span>Base</span>
          <code className="text-foreground/80">{shortSha(artifact.pr.base_sha)}</code>
          <span aria-hidden>→</span>
          <span>Head</span>
          <code className="text-foreground/80">{shortSha(artifact.pr.head_sha)}</code>
        </div>
      )}
    </header>
  );
}
