export async function enqueue(id: string): Promise<void> {
  await send(id);
}

async function send(id: string): Promise<void> {
  console.log("send", id);
}
