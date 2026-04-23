import type { Artifact } from "@/types/artifact";
import { deriveSlug, isPathSha, shortSha } from "@/lib/artifact";

/** PR header — reviewer-facing title first, identity metadata second.
 *
 *  When the LLM summary pass has produced a headline, it gets the
 *  primary heading slot (18px semibold) with an optional 1-sentence
 *  description below. When no headline is available, the heading IS
 *  the identity (repo#N or derived slug) — we deliberately avoid
 *  echoing the same string twice, which made earlier revisions read
 *  like debug output. Base→head sha chips live below the heading in
 *  either case for cite-back, always mono + small so they never
 *  compete with the headline.
 */
export function PrHeader({ artifact }: { artifact: Artifact }) {
  const slug = deriveSlug(artifact.pr.base_sha, artifact.pr.head_sha);
  const showSha = !isPathSha(artifact.pr.base_sha) && !isPathSha(artifact.pr.head_sha);
  const summary = artifact.pr_summary ?? null;
  const identityIsHeading = !summary?.headline;
  // When the repo is a sample fixture or "unknown", the derived slug
  // is the only sensible identity — use it as the heading fallback.
  // For real GitHub repos the `owner/name` is the heading fallback,
  // slug demotes to metadata.
  const heading = summary?.headline
    ?? (artifact.pr.repo === "unknown" || artifact.pr.repo.startsWith("sample/")
        ? slug
        : artifact.pr.repo);

  return (
    <header className="space-y-1.5">
      {summary?.headline ? (
        <h1 className="text-[18px] font-semibold text-foreground leading-tight">
          {heading}
        </h1>
      ) : (
        <h1 className="text-[16px] font-mono font-semibold text-foreground">
          {heading}
        </h1>
      )}
      {summary?.description && (
        <p className="text-[13px] text-muted-foreground max-w-3xl leading-relaxed">
          {summary.description}
        </p>
      )}
      {(showSha || !identityIsHeading) && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 pt-0.5 text-[10px] font-mono text-muted-foreground">
          {/* When the LLM headline took the primary slot, surface the
              slug here as the reviewer-facing identity chip. When the
              identity IS the heading, skip this to avoid duplication. */}
          {!identityIsHeading && (
            <span>{slug}</span>
          )}
          {showSha && (
            <>
              {!identityIsHeading && <span aria-hidden className="opacity-40">·</span>}
              <span>
                base <code className="text-foreground/70">{shortSha(artifact.pr.base_sha)}</code>
              </span>
              <span aria-hidden className="opacity-40">→</span>
              <span>
                head <code className="text-foreground/70">{shortSha(artifact.pr.head_sha)}</code>
              </span>
            </>
          )}
        </div>
      )}
    </header>
  );
}
