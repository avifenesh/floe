import type { Artifact } from "./types/artifact";

export type JobStatus =
  | { status: "pending" }
  | { status: "ready" }
  | { status: "error"; message: string };

export interface JobView {
  status: JobStatus["status"];
  message?: string;
  artifact?: Artifact;
}

export async function analyze(basePath: string, headPath: string): Promise<string> {
  const r = await fetch("/analyze", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ base_path: basePath, head_path: headPath }),
  });
  if (!r.ok) throw new Error(`analyze failed: ${r.status} ${await r.text()}`);
  const j = (await r.json()) as { job_id: string };
  return j.job_id;
}

export async function getJob(jobId: string): Promise<JobView> {
  const r = await fetch(`/analyze/${jobId}`);
  if (!r.ok) throw new Error(`getJob failed: ${r.status}`);
  return (await r.json()) as JobView;
}

/** Poll /analyze/:id until status leaves pending. Cheap 200ms cadence — the
 *  SSE stream is wired in as soon as we start rendering progress. */
export async function pollUntilDone(jobId: string, signal?: AbortSignal): Promise<JobView> {
  while (true) {
    if (signal?.aborted) throw new Error("aborted");
    const v = await getJob(jobId);
    if (v.status !== "pending") return v;
    await new Promise((res) => setTimeout(res, 200));
  }
}
