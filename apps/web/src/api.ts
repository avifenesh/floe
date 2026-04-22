import type { Artifact } from "./types/artifact";

/** Dev-only: when the Vite dev server is on :5173, all API calls go
 *  to the backend on :8787 (different origin → credentialed requests
 *  need explicit origin). In production the FE is served from the
 *  same origin as the backend, so this is empty and requests are
 *  same-origin. Declared at the top so every fetch call below can
 *  use it without triggering the TDZ.
 */
const BACKEND_BASE =
  typeof window !== "undefined" && window.location.port === "5173"
    ? "http://127.0.0.1:8787"
    : "";

export type JobStatus =
  | { status: "pending" }
  | { status: "ready" }
  | { status: "error"; message: string };

export interface JobView {
  status: JobStatus["status"];
  message?: string;
  artifact?: Artifact;
}

/** Response shape from POST /analyze/url — in addition to the job id,
 *  the server surfaces the resolved PR coordinates so the UI can show
 *  "glide-mq#181 · base 2e6aadc → head a1b2c3d" without a second call. */
export interface AnalyzeUrlResult {
  job_id: string;
  repo: string;
  pr_number: number;
  base_sha: string;
  head_sha: string;
}

export async function analyzeUrl(url: string): Promise<AnalyzeUrlResult> {
  const r = await fetch(`${BACKEND_BASE}/analyze/url`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ url }),
    credentials: "include",
  });
  if (!r.ok) throw new Error(`analyzeUrl failed: ${r.status} ${await r.text()}`);
  return (await r.json()) as AnalyzeUrlResult;
}

export async function analyze(basePath: string, headPath: string): Promise<string> {
  const r = await fetch(`${BACKEND_BASE}/analyze`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ base_path: basePath, head_path: headPath }),
    credentials: "include",
  });
  if (!r.ok) throw new Error(`analyze failed: ${r.status} ${await r.text()}`);
  const j = (await r.json()) as { job_id: string };
  return j.job_id;
}

export async function getJob(jobId: string): Promise<JobView> {
  const r = await fetch(`${BACKEND_BASE}/analyze/${jobId}`, {
    credentials: "include",
  });
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

/** Row in the landing-page PR history list. Mirrors the server's
 *  `AnalysisRow` — kept hand-typed because it crosses language
 *  boundaries at a different cadence than the schemars-generated
 *  `Artifact` type (schema there bumps on artifact changes, not
 *  history changes). */
export interface AnalysisRow {
  id: string;
  user_id?: string;
  repo?: string;
  pr_number?: number;
  head_sha: string;
  intent_fp: string;
  llm_sig: string;
  artifact_key?: string;
  status: "pending" | "ready" | "errored";
  message?: string;
  created_at: string;
  updated_at: string;
}

export async function listPrAnalyses(limit = 50): Promise<AnalysisRow[]> {
  const r = await fetch(`${BACKEND_BASE}/analyses?limit=${limit}`, {
    credentials: "include",
  });
  if (!r.ok) throw new Error(`listPrAnalyses failed: ${r.status}`);
  return (await r.json()) as AnalysisRow[];
}

export async function deleteAnalysis(id: string): Promise<void> {
  const r = await fetch(`${BACKEND_BASE}/analyses/${id}`, {
    method: "DELETE",
    credentials: "include",
  });
  if (!r.ok) throw new Error(`deleteAnalysis failed: ${r.status}`);
}

/** Who's signed in. Returns null when no session cookie is present
 *  (backend responds 401). Called once on landing-page mount. */
export interface Me {
  id: string;
  provider: string;
  provider_user_id: string;
  email?: string;
  display_name?: string;
  avatar_url?: string;
  created_at: string;
}

export async function fetchMe(): Promise<Me | null> {
  const r = await fetch(`${BACKEND_BASE}/me`, { credentials: "include" });
  if (r.status === 401) return null;
  if (!r.ok) throw new Error(`fetchMe failed: ${r.status}`);
  return (await r.json()) as Me;
}

/** Dev-only: hit POST /auth/dev/login to fake a session under the
 *  `dev` provider. The server gates this route behind
 *  `ADR_ALLOW_DEV_LOGIN=1`; if disabled, this returns a 404 we
 *  surface as a friendly error. */
export async function devLogin(handle: string): Promise<Me> {
  const r = await fetch(`${BACKEND_BASE}/auth/dev/login`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ handle }),
    credentials: "include",
  });
  if (r.status === 404) {
    throw new Error("dev login disabled — set ADR_ALLOW_DEV_LOGIN=1 on the server");
  }
  if (!r.ok) throw new Error(`devLogin failed: ${r.status} ${await r.text()}`);
  return (await r.json()) as Me;
}

export async function logout(): Promise<void> {
  await fetch(`${BACKEND_BASE}/auth/logout`, {
    method: "POST",
    credentials: "include",
  });
}

/** Absolute URL to kick off the GitHub sign-in flow. The browser
 *  navigates here; the server 307's to GitHub, then back to our
 *  callback, then back to the FE. */
export const githubLoginUrl = `${BACKEND_BASE || "http://127.0.0.1:8787"}/auth/github`;

/** Fetch the raw bytes of a file from the job's base or head snapshot. */
export async function fetchFile(
  jobId: string,
  side: "base" | "head",
  path: string,
): Promise<string> {
  const q = new URLSearchParams({ side, path });
  const r = await fetch(`${BACKEND_BASE}/analyze/${jobId}/file?${q}`, {
    credentials: "include",
  });
  if (!r.ok) throw new Error(`fetchFile(${side}, ${path}): ${r.status} ${await r.text()}`);
  return r.text();
}

/** What the server's env currently resolves to for the three LLM
 *  passes. Drives the "re-baseline required" banner on an artifact
 *  whose pin disagrees with the current config (RFC v0.3 §9). */
export interface LlmConfigView {
  synthesis_model?: string;
  probe_model?: string;
  proof_model?: string;
}

/** GET /llm-config — model names only, no keys/URLs. */
export async function fetchLlmConfig(): Promise<LlmConfigView> {
  const r = await fetch(`${BACKEND_BASE}/llm-config`, {
    credentials: "include",
  });
  if (!r.ok) throw new Error(`fetchLlmConfig: ${r.status}`);
  return r.json();
}
