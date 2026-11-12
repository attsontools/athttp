import { describe, expect, it } from 'vitest';
import { mergeIntoCollection } from './mergeIntoCollection';
import { makeEmptySpec } from './types';
import type { HttpCollection, HttpCollectionFolder, HttpCollectionRequest, HttpRequestSpec } from './types';
import type { ImportedOpenApiCollection } from './openapiImport';

function req(overrides: Partial<HttpCollectionRequest>): HttpCollectionRequest {
  return {
    id: 'r?',
    collectionId: 'c1',
    folderId: null,
    name: 'req',
    method: 'GET',
    spec: makeEmptySpec(),
    orderIndex: 0,
    updatedAt: 1,
    sourceKey: null,
    ...overrides
  };
}

function specWith(url: string, extra?: Partial<HttpRequestSpec>): HttpRequestSpec {
  return { ...makeEmptySpec(), url, ...extra };
}

const baseCollection: HttpCollection = {
  id: 'c1',
  name: 'A',
  orderIndex: 0,
  updatedAt: 1,
  sourceUrl: 'https://example/openapi.json'
};

function incoming(reqs: Array<{ method: string; path: string; url: string }>): ImportedOpenApiCollection {
  return {
    baseUrl: '',
    collection: { id: 'ignored', name: 'A', orderIndex: 0 },
    folders: [],
    requests: reqs.map((r, i) => ({
      id: `new-${i}`,
      collectionId: 'ignored',
      folderId: null,
      name: `${r.method} ${r.path}`,
      method: r.method as HttpCollectionRequest['method'],
      spec: specWith(r.url),
      orderIndex: i,
      sourceKey: `${r.method} ${r.path}`
    }))
  };
}

describe('mergeIntoCollection', () => {
  it('全新集合：incoming 全部 added', () => {
    const result = mergeIntoCollection(
      { collection: baseCollection, folders: [], requests: [] },
      incoming([
        { method: 'GET', path: '/users', url: 'https://api/users' },
        { method: 'POST', path: '/users', url: 'https://api/users' }
      ])
    );
    expect(result.diff).toEqual({ added: 2, updated: 0, deleted: 0 });
    expect(result.requests).toHaveLength(2);
    expect(result.requests.every((r) => r.sourceKey)).toBe(true);
  });

  it('用户手加请求（sourceKey=null）在 sync 后完全保留', () => {
    const existing = [
      req({ id: 'r-user', sourceKey: null, name: 'my custom', spec: specWith('/custom') })
    ];
    const result = mergeIntoCollection(
      { collection: baseCollection, folders: [], requests: existing },
      incoming([{ method: 'GET', path: '/users', url: 'https://api/users' }])
    );
    expect(result.requests.find((r) => r.id === 'r-user')).toEqual(existing[0]);
    expect(result.diff.added).toBe(1);
    expect(result.diff.deleted).toBe(0);
  });

  it('已存在的受管请求：spec 更新，name 跟着 incoming 覆盖', () => {
    const existing = [
      req({
        id: 'r-1',
        sourceKey: 'GET /users',
        name: '我的用户列表',
        spec: specWith('https://old/users', { timeoutSeconds: 60, saveToHistory: false })
      })
    ];
    const result = mergeIntoCollection(
      { collection: baseCollection, folders: [], requests: existing },
      incoming([{ method: 'GET', path: '/users', url: 'https://api/users' }])
    );
    const updated = result.requests.find((r) => r.id === 'r-1')!;
    expect(updated.name).toBe('GET /users');
    expect(updated.spec.url).toBe('https://api/users');
    expect(updated.spec.timeoutSeconds).toBe(60);
    expect(updated.spec.saveToHistory).toBe(false);
    expect(result.diff).toEqual({ added: 0, updated: 1, deleted: 0 });
  });

  it('已存在的受管请求：name 总是取自 incoming', () => {
    const existing = [
      req({
        id: 'r-1',
        sourceKey: 'GET /users',
        name: 'GET /users',
        spec: specWith('https://old/users')
      })
    ];
    const result = mergeIntoCollection(
      { collection: baseCollection, folders: [], requests: existing },
      incoming([{ method: 'GET', path: '/users', url: 'https://api/users' }])
    );
    expect(result.requests.find((r) => r.id === 'r-1')!.name).toBe('GET /users');
  });

  it('incoming 里请求消失 → 删除受管请求', () => {
    const existing = [
      req({ id: 'r-gone', sourceKey: 'GET /old', name: 'old' })
    ];
    const result = mergeIntoCollection(
      { collection: baseCollection, folders: [], requests: existing },
      incoming([])
    );
    expect(result.requests).toHaveLength(0);
    expect(result.deletedRequestIds).toEqual(['r-gone']);
    expect(result.diff).toEqual({ added: 0, updated: 0, deleted: 1 });
  });

  it('folder 里所有受管请求被删且无用户自加 → folder 也删', () => {
    const existing = {
      collection: baseCollection,
      folders: [{ id: 'f1', collectionId: 'c1', parentId: null, name: 'users', orderIndex: 0, updatedAt: 1 }],
      requests: [req({ id: 'r-gone', folderId: 'f1', sourceKey: 'GET /users' })]
    };
    const result = mergeIntoCollection(existing, incoming([]));
    expect(result.folders).toHaveLength(0);
    expect(result.deletedFolderIds).toEqual(['f1']);
  });

  it('folder 里还留有用户自加请求 → folder 保留', () => {
    const existing = {
      collection: baseCollection,
      folders: [{ id: 'f1', collectionId: 'c1', parentId: null, name: 'users', orderIndex: 0, updatedAt: 1 }],
      requests: [
        req({ id: 'r-gone', folderId: 'f1', sourceKey: 'GET /users' }),
        req({ id: 'r-user', folderId: 'f1', sourceKey: null })
      ]
    };
    const result = mergeIntoCollection(existing, incoming([]));
    expect(result.folders).toHaveLength(1);
    expect(result.deletedFolderIds).toEqual([]);
  });

  it('用户移动过 folder → 同步后保留移后 folder', () => {
    const existing = {
      collection: baseCollection,
      folders: [
        { id: 'f1', collectionId: 'c1', parentId: null, name: 'users', orderIndex: 0, updatedAt: 1 },
        { id: 'f-target', collectionId: 'c1', parentId: null, name: 'admin', orderIndex: 1, updatedAt: 1 }
      ],
      requests: [
        req({ id: 'r-moved', folderId: 'f-target', sourceKey: 'GET /users', name: 'GET /users' })
      ]
    };
    const result = mergeIntoCollection(existing, incoming([{ method: 'GET', path: '/users', url: 'https://api/users' }]));
    expect(result.requests.find((r) => r.id === 'r-moved')!.folderId).toBe('f-target');
  });
});
