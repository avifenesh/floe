import { useState } from "react";
import { TopSpine } from "@/components/TopSpine";
import { PrView } from "@/views/pr";
import { SourceView } from "@/views/source";
import { PlaceholderView } from "@/views/placeholder";
import type { ViewKey } from "@/views/types";
import type { Artifact } from "@/types/artifact";
import { deriveSlug, isPathSha, shortSha } from "@/lib/artifact";

export interface LoadedJob {
  jobId: string;
  artifact: Artifact;
}

export default function App() {
  const [view, setView] = useState<ViewKey>("pr");
  const [job, setJob] = useState<LoadedJob | null>(null);

  const prLabel = job ? spineLabel(job.artifact) : null;

  return (
    <div className="min-h-screen flex flex-col">
      <TopSpine view={view} onView={setView} prLabel={prLabel} />
      <main className="flex-1 w-full max-w-6xl mx-auto px-6 pt-4 pb-10">
        {view === "pr" ? (
          <PrView job={job} onJob={setJob} />
        ) : view === "source" && job ? (
          <SourceView artifact={job.artifact} jobId={job.jobId} />
        ) : (
          <PlaceholderView view={view} />
        )}
      </main>
    </div>
  );
}

/** Spine identity: real `repo · sha` when we have them, otherwise the
 *  fixture slug derived from common-parent of base/head paths. */
function spineLabel(a: Artifact): string {
  if (a.pr.repo !== "unknown" && !isPathSha(a.pr.head_sha)) {
    return `${a.pr.repo} · ${shortSha(a.pr.head_sha)}`;
  }
  return deriveSlug(a.pr.base_sha, a.pr.head_sha);
}
