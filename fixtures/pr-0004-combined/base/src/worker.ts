export type WorkerStatus = "idle" | "busy";

export async function runJob(id: string): Promise<void> {
  await execute(id);
}

async function execute(id: string): Promise<void> {
  console.log("run", id);
}
