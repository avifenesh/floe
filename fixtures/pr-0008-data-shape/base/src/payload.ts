export interface JobPayload {
  id: string;
  body: string;
  createdAt: number;
}

export function isJobPayload(v: unknown): v is JobPayload {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return typeof o.id === "string" && typeof o.body === "string";
}
