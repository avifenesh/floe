export interface JobPayload {
  jobId: string;
  body: string;
  createdAt: number;
  retries: number;
  deadline: number;
}

export function isJobPayload(v: unknown): v is JobPayload {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return (
    typeof o.jobId === "string" &&
    typeof o.body === "string" &&
    typeof o.retries === "number" &&
    typeof o.deadline === "number"
  );
}
