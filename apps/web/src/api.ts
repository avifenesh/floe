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

/** Fetch the raw bytes of a file from the job's base or head snapshot. */
export async function fetchFile(
  jobId: string,
  side: "base" | "head",
  path: string,
): Promise<string> {
  const u = new URL(`/analyze/${jobId}/file`, window.location.origin);
  u.searchParams.set("side", side);
  u.searchParams.set("path", path);
  const r = await fetch(u.pathname + u.search);
  if (!r.ok) throw new Error(`fetchFile(${side}, ${path}): ${r.status} ${await r.text()}`);
  return r.text();
}
