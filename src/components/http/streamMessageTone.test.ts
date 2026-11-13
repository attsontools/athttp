import { describe, it, expect } from 'vitest';
import { messageTone } from './streamMessageTone';

describe('messageTone', () => {
  it.each([
    ['open', 'ok'],
    ['closed', 'muted'],
    ['error', 'err'],
    ['bufferTruncated', 'warn'],
    ['wsBinary', 'info'],
    ['sseEvent', 'default'],
    ['wsText', 'default'],
  ])('%s → %s', (kind, tone) => {
    const m = { kind, atMs: 0 } as any;
    expect(messageTone(m)).toBe(tone);
  });
});
