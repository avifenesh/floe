export class Queue {
  private buffer: string[] = [];

  enqueue(item: string): void {
    this.buffer.push(item);
  }

  async flush(sink: (items: string[]) => Promise<void>): Promise<void> {
    const snapshot = this.buffer.slice();
    this.buffer = [];
    await sink(snapshot);
  }
}
