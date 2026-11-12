import type { StreamMessage } from './types';

export interface ServiceEvent {
  sessionId: string;
  generation: number;
  message: StreamMessage;
}

export interface ServiceTransport {
  request<T>(method: string, params?: unknown): Promise<T>;
  subscribe(listener: (event: ServiceEvent) => void): () => void;
  close(): void;
}
