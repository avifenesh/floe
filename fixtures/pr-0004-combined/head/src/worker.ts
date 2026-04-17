export type WorkerStatus = "idle" | "busy" | "retrying" | "failed";

export async function runJob(id: string, maxAttempts: number): Promise<void> {
  await executeWithRetry(id, maxAttempts);
}

async function executeWithRetry(id: string, attempts: number): Promise<void> {
  for (let i = 0; i < attempts; i++) {
    try {
      await execute(id);
      return;
    } catch (_err) {
      continue;
    }
  }
}

async function execute(id: string): Promise<void> {
  console.log("run", id);
}
