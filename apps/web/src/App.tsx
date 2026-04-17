import { useState } from "react";
import { TopSpine } from "@/components/TopSpine";
import { PrView } from "@/views/pr";
import { PlaceholderView } from "@/views/placeholder";
import type { ViewKey } from "@/views/types";
import type { Artifact } from "@/types/artifact";

export default function App() {
  const [view, setView] = useState<ViewKey>("pr");
  const [artifact, setArtifact] = useState<Artifact | null>(null);

  const prLabel = artifact
    ? `${artifact.pr.repo} · ${short(artifact.pr.head_sha)}`
    : null;

  return (
    <div className="min-h-screen flex flex-col">
      <TopSpine view={view} onView={setView} prLabel={prLabel} />
      <main className="flex-1 w-full max-w-6xl mx-auto px-6 pt-4 pb-10">
        {view === "pr" ? (
          <PrView artifact={artifact} onArtifact={setArtifact} />
        ) : (
          <PlaceholderView view={view} />
        )}
      </main>
    </div>
  );
}

/** Last path segment, trimmed for the spine label. */
function short(sha: string) {
  const s = sha.replace(/\\/g, "/");
  const seg = s.split("/").pop() ?? s;
  return seg.length > 24 ? seg.slice(0, 24) + "…" : seg;
}
