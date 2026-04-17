export type Job = {
  id: string;
  payload: unknown;
  attempts: number;
};

export async function enqueue(job: Job): Promise<void> {
  await sendWithRetry(job);
}

async function sendWithRetry(job: Job): Promise<void> {
  for (let i = 0; i < 3; i++) {
    try {
      await send(job);
      return;
    } catch (_err) {
      continue;
    }
  }
}

async function send(job: Job): Promise<void> {
  console.log("send", job.id);
}
