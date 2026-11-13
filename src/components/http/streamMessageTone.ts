import type { StreamMessage } from './types';

export type MessageTone = 'ok' | 'warn' | 'err' | 'muted' | 'info' | 'default';

export function messageTone(m: StreamMessage): MessageTone {
  switch (m.kind) {
    case 'open': return 'ok';
    case 'closed': return 'muted';
    case 'error': return 'err';
    case 'bufferTruncated': return 'warn';
    case 'wsBinary': return 'info';
    case 'sseEvent':
    case 'wsText':
    default: return 'default';
  }
}
