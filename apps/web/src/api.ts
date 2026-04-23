import type { Artifact } from "./types/artifact";

/** Dev: vite on :5173 talks to the backend on :8787. Cross-origin,
 *  but the session cookie is set with `SameSite=None; Secure` so it
 *  travels on credentialed XHR (127.0.0.1 counts as a secure context
 *  for browser cookie policy even over HTTP). Prod: same-origin, so
 *  BACKEND_BASE is empty.
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

export async function analyze(
  basePath: string,
  headPath: string,
  intent?: unknown,
): Promise<string> {
  const body: Record<string, unknown> = { base_path: basePath, head_path: headPath };
  if (intent !== undefined) body.intent = intent;
  const r = await fetch(`${BACKEND_BASE}/analyze`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    credentials: "include",
  });
  if (!r.ok) throw new Error(`analyze failed: ${r.status} ${await r.text()}`);
  const j = (await r.json()) as { job_id: string };
  return j.job_id;
}

/** One demo PR the landing gallery offers. See `crates/floe-server/src/samples.rs`. */
export interface SampleView {
  id: string;
  title: string;
  description: string;
}

/** GET /samples — list of built-in demo PRs. Returns `[]` when the
 *  server started without a fixtures root (bare-bones deploy). */
export async function fetchSamples(): Promise<SampleView[]> {
  const r = await fetch(`${BACKEND_BASE}/samples`, {
    credentials: "include",
  });
  if (!r.ok) throw new Error(`fetchSamples: ${r.status}`);
  return (await r.json()) as SampleView[];
}

/** POST /analyze/sample/:id — kick off analysis on one of the
 *  built-in samples. Server resolves the paths; the client never
 *  sees or sends them. */
export async function analyzeSample(sampleId: string): Promise<string> {
  const r = await fetch(`${BACKEND_BASE}/analyze/sample/${encodeURIComponent(sampleId)}`, {
    method: "POST",
    credentials: "include",
  });
  if (!r.ok) throw new Error(`analyzeSample failed: ${r.status} ${await r.text()}`);
  const j = (await r.json()) as { job_id: string };
  return j.job_id;
}

/** POST /analyze/:id/rebaseline — spawn a fresh analysis for the
 *  same logical PR under the current LLM regime. The server looks
 *  up the cached artifact and replays it against whichever source
 *  it can (sample table for sample runs, git_sync checkouts for
 *  GitHub URL runs). Path-driven artifacts 400 with a hint. */
export async function rebaselineJob(jobId: string): Promise<string> {
  const r = await fetch(`${BACKEND_BASE}/analyze/${encodeURIComponent(jobId)}/rebaseline`, {
    method: "POST",
    credentials: "include",
  });
  if (!r.ok) throw new Error(`rebaseline failed: ${r.status} ${await r.text()}`);
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
 *  `FLOE_ALLOW_DEV_LOGIN=1`; if disabled, this returns a 404 we
 *  surface as a friendly error. */
export async function devLogin(handle: string): Promise<Me> {
  const r = await fetch(`${BACKEND_BASE}/auth/dev/login`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ handle }),
    credentials: "include",
  });
  if (r.status === 404) {
    throw new Error("dev login disabled — set FLOE_ALLOW_DEV_LOGIN=1 on the server");
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

// ─────────────────────────────────────────────────────────────────────
// Inline notes — reviewer annotations anchored to artifact objects.
// Mirrors `floe_core::inline_notes`.
// ─────────────────────────────────────────────────────────────────────

export type InlineNoteAnchor =
  | { kind: "hunk"; hunk_id: string }
  | { kind: "flow"; flow_id: string }
  | { kind: "entity"; entity_name: string }
  | { kind: "intent-claim"; claim_index: number }
  | { kind: "file-line"; file: string; line_side: "base" | "head"; line: number };

export interface InlineNote {
  id: string;
  anchor: InlineNoteAnchor;
  text: string;
  author: string;
  created_at: string;
}

export async function addInlineNote(
  jobId: string,
  anchor: InlineNoteAnchor,
  text: string,
): Promise<InlineNote> {
  const r = await fetch(`${BACKEND_BASE}/analyze/${jobId}/notes`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ anchor, text }),
    credentials: "include",
  });
  if (!r.ok) throw new Error(`addInlineNote: ${r.status} ${await r.text()}`);
  return r.json();
}

export async function deleteInlineNote(jobId: string, noteId: string): Promise<void> {
  const r = await fetch(
    `${BACKEND_BASE}/analyze/${jobId}/notes/${encodeURIComponent(noteId)}`,
    { method: "DELETE", credentials: "include" },
  );
  if (!r.ok) throw new Error(`deleteInlineNote: ${r.status}`);
}

/** Returns the export bundle suitable for pasting into a coding agent. */
// ─────────────────────────────────────────────────────────────────────
// Review verdict — server-persisted reviewer stance (approve /
// request-changes / comment). Mirrors floe_core::ReviewVerdictRecord.
// ─────────────────────────────────────────────────────────────────────

export type ReviewVerdict = "approve" | "request-changes" | "comment";

export interface ReviewVerdictRecord {
  verdict: ReviewVerdict;
  author: string;
  set_at: string;
}

export async function setReviewVerdict(
  jobId: string,
  verdict: ReviewVerdict,
): Promise<ReviewVerdictRecord> {
  const r = await fetch(`${BACKEND_BASE}/analyze/${jobId}/verdict`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ verdict }),
    credentials: "include",
  });
  if (!r.ok) throw new Error(`setReviewVerdict: ${r.status} ${await r.text()}`);
  return r.json();
}

export interface CompareResponse {
  a: CompareSide;
  b: CompareSide;
  pin_matches: boolean;
  aggregate_delta: {
    continuation: number;
    runtime: number;
    operational: number;
    tokens: number;
  } | null;
  flows: CompareFlow[];
}

export interface CompareSide {
  id: string;
  repo: string;
  head_sha: string;
  headline: string | null;
  synth_status: string;
  proof_status: string;
  cost_status: string;
  flow_count: number;
  hunk_count: number;
  baseline: unknown | null;
  verdict: ReviewVerdictRecord | null;
}

export interface CompareFlow {
  name: string;
  presence: "both" | "only-a" | "only-b";
  a: CompareFlowSide | null;
  b: CompareFlowSide | null;
}

export interface CompareFlowSide {
  intent_fit: string | null;
  proof: string | null;
  cost_net: number | null;
}

export async function compareAnalyses(
  aId: string,
  bId: string,
): Promise<CompareResponse> {
  const r = await fetch(
    `${BACKEND_BASE}/compare/${encodeURIComponent(aId)}/${encodeURIComponent(bId)}`,
    { credentials: "include" },
  );
  if (!r.ok) throw new Error(`compareAnalyses: ${r.status} ${await r.text()}`);
  return r.json();
}

export async function clearReviewVerdict(jobId: string): Promise<void> {
  const r = await fetch(`${BACKEND_BASE}/analyze/${jobId}/verdict`, {
    method: "DELETE",
    credentials: "include",
  });
  if (!r.ok) throw new Error(`clearReviewVerdict: ${r.status}`);
}

export async function exportInlineNotes(jobId: string): Promise<unknown> {
  const r = await fetch(`${BACKEND_BASE}/analyze/${jobId}/notes/export`, {
    credentials: "include",
  });
  if (!r.ok) throw new Error(`exportInlineNotes: ${r.status}`);
  return r.json();
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
