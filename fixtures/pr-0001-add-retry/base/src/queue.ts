export type Job = {
  id: string;
  payload: unknown;
};

export async function enqueue(job: Job): Promise<void> {
  await send(job);
}

async function send(job: Job): Promise<void> {
  console.log("send", job.id);
}
