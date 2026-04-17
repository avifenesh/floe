export async function enqueue(id: string): Promise<void> {
  await send(id);
}

export async function enqueueBatch(ids: string[]): Promise<void> {
  for (const id of ids) {
    await send(id);
  }
}

async function send(id: string): Promise<void> {
  console.log("send", id);
}
