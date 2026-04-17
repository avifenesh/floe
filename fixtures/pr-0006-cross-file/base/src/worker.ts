import { enqueue } from "./queue";

export async function runJob(id: string): Promise<void> {
  await enqueue(id);
}
