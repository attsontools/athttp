import { describe, it, expect } from 'vitest';
import { folderHasVisibleContent, folderRequestCount } from './collectionTree';
import type { HttpCollectionFolder } from './types';

function folder(id: string, parentId: string | null): HttpCollectionFolder {
  return { id, collectionId: 'c1', parentId, name: id, orderIndex: 0, updatedAt: 0 };
}

describe('folderHasVisibleContent', () => {
  // 树: platform > [source, sink];  另有独立的 empty
  const platform = folder('platform', null);
  const source = folder('source', 'platform');
  const sink = folder('sink', 'platform');
  const empty = folder('empty', null);
  const all = [platform, source, sink, empty];

  it('is true when the folder itself has direct requests', () => {
    const count = (_c: string, fid: string) => (fid === 'platform' ? 2 : 0);
    expect(folderHasVisibleContent(platform, 'c1', all, count)).toBe(true);
  });

  it('is true when a descendant has requests even if the folder is empty', () => {
    const count = (_c: string, fid: string) => (fid === 'source' ? 1 : 0);
    expect(folderHasVisibleContent(platform, 'c1', all, count)).toBe(true);
  });

  it('is false when neither the folder nor any descendant has requests', () => {
    const count = () => 0;
    expect(folderHasVisibleContent(platform, 'c1', all, count)).toBe(false);
    expect(folderHasVisibleContent(empty, 'c1', all, count)).toBe(false);
  });

  it('only counts descendants within the same collection', () => {
    const foreign = folder('foreign', 'platform');
    foreign.collectionId = 'other';
    const count = (_c: string, fid: string) => (fid === 'foreign' ? 5 : 0);
    // foreign 属于别的 collection,不应让 platform 判定为有内容
    expect(folderHasVisibleContent(platform, 'c1', [...all, foreign], count)).toBe(false);
  });
});

describe('folderRequestCount', () => {
  const platform = folder('platform', null);
  const source = folder('source', 'platform');
  const sink = folder('sink', 'platform');
  const all = [platform, source, sink];

  it('sums direct and descendant request counts', () => {
    const count = (_c: string, fid: string) =>
      ({ platform: 1, source: 3, sink: 2 } as Record<string, number>)[fid] ?? 0;
    // 1 (直属) + 3 (source) + 2 (sink) = 6
    expect(folderRequestCount(platform, 'c1', all, count)).toBe(6);
    expect(folderRequestCount(source, 'c1', all, count)).toBe(3);
  });

  it('returns 0 for an empty folder subtree', () => {
    expect(folderRequestCount(platform, 'c1', all, () => 0)).toBe(0);
  });
});
