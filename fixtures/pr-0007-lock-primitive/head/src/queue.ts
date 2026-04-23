import { Mutex } from "async-mutex";

export class Queue {
  private buffer: string[] = [];
  private flushLock = new Mutex();

  enqueue(item: string): void {
    this.buffer.push(item);
  }

  async flush(sink: (items: string[]) => Promise<void>): Promise<void> {
    const release = await this.flushLock.acquire();
    try {
      const snapshot = this.buffer.slice();
      this.buffer = [];
      await sink(snapshot);
    } finally {
      release();
    }
  }
}
