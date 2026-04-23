/**
 * Fetch a batch of items by id.
 * @param id the collection id
 * @param count how many items to fetch
 */
export async function fetchBatch(id: string, count: number): Promise<string[]> {
  const out: string[] = [];
  for (let i = 0; i < count; i++) {
    out.push(`${id}-${i}`);
  }
  return out;
}
