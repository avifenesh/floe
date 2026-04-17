import { enqueueBatch } from "./queue";

export async function runJob(ids: string[]): Promise<void> {
  await enqueueBatch(ids);
}
