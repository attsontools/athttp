import type { OpenApiWorkerReq, OpenApiWorkerRes } from '../workers/openapiWorker';
import type { OpenApiImportOptions, ImportedOpenApiCollection } from '../components/http/openapiImport';

export type OpenApiParseOutcome =
  | { ok: true; result: ImportedOpenApiCollection; elapsedMs: number }
  | { ok: false; error: string };

export type OpenApiFormatOutcome =
  | { ok: true; text: string; elapsedMs: number }
  | { ok: false; error: string };

export interface OpenApiWorkerLike {
  postMessage(msg: OpenApiWorkerReq): void;
  addEventListener(type: 'message', listener: (ev: { data: OpenApiWorkerRes }) => void): void;
  terminate(): void;
}

export interface OpenApiImportWorkerClient {
  parse(text: string, options: OpenApiImportOptions, tag?: string): Promise<OpenApiParseOutcome | null>;
  format(text: string, indent?: number, tag?: string): Promise<OpenApiFormatOutcome | null>;
  dispose(): void;
}

type Pending = { complete: (res: OpenApiWorkerRes | null) => void };

export function createOpenApiImportWorkerClient(
  workerFactory: () => OpenApiWorkerLike,
): OpenApiImportWorkerClient {
  const worker = workerFactory();
  const pending = new Map<number, Pending>();
  const cancelledIds = new Set<number>();
  const tagToId = new Map<string, number>();
  let nextId = 1;

  worker.addEventListener('message', (ev) => {
    const res = ev.data;
    const p = pending.get(res.id);
    if (!p) return;
    pending.delete(res.id);
    if (cancelledIds.has(res.id)) {
      cancelledIds.delete(res.id);
      p.complete(null);
      return;
    }
    p.complete(res);
  });

  return {
    parse(text, options, tag) {
      const id = nextId++;
      if (tag) {
        const prevId = tagToId.get(tag);
        if (prevId !== undefined && pending.has(prevId)) {
          cancelledIds.add(prevId);
        }
        tagToId.set(tag, id);
      }
      return new Promise((resolve) => {
        pending.set(id, {
          complete(res) {
            if (res === null) {
              resolve(null);
            } else if (res.kind !== 'parse') {
              resolve({
                ok: false,
                error: `OpenAPI Worker 响应类型不匹配：期望 parse，收到 ${res.kind}`,
              });
            } else if (res.ok) {
              resolve({ ok: true, result: res.result, elapsedMs: res.elapsedMs });
            } else {
              resolve({ ok: false, error: res.error });
            }
          },
        });
        worker.postMessage({ id, kind: 'parse', text, options });
      });
    },
    format(text, indent = 2, tag) {
      const id = nextId++;
      if (tag) {
        const prevId = tagToId.get(tag);
        if (prevId !== undefined && pending.has(prevId)) {
          cancelledIds.add(prevId);
        }
        tagToId.set(tag, id);
      }
      return new Promise((resolve) => {
        pending.set(id, {
          complete(res) {
            if (res === null) {
              resolve(null);
            } else if (res.kind !== 'format') {
              resolve({
                ok: false,
                error: `OpenAPI Worker 响应类型不匹配：期望 format，收到 ${res.kind}`,
              });
            } else if (res.ok) {
              resolve({ ok: true, text: res.text, elapsedMs: res.elapsedMs });
            } else {
              resolve({ ok: false, error: res.error });
            }
          },
        });
        worker.postMessage({ id, kind: 'format', text, indent });
      });
    },
    dispose() {
      worker.terminate();
      pending.clear();
      cancelledIds.clear();
      tagToId.clear();
    },
  };
}

let singleton: OpenApiImportWorkerClient | null = null;

export function useOpenApiImportWorker(): OpenApiImportWorkerClient {
  if (!singleton) {
    singleton = createOpenApiImportWorkerClient(() =>
      new Worker(
        new URL('../workers/openapiWorker.ts', import.meta.url),
        { type: 'module' },
      ) as unknown as OpenApiWorkerLike,
    );
  }
  return singleton;
}
