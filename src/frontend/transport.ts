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

export function formatServiceError(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null) {
    const value = error as { code?: unknown; message?: unknown };
    if (typeof value.message === 'string') {
      return typeof value.code === 'string' ? `${value.code}: ${value.message}` : value.message;
    }
  }
  return String(error);
}
