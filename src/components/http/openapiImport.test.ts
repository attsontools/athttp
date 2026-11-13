import { describe, expect, it } from 'vitest';
import { parseOpenApiToCollection } from './openapiImport';

const spec = {
  openapi: '3.0.3',
  info: { title: 'Petstore Admin', version: '1.0.0' },
  servers: [{ url: 'https://api.example.com/v1' }],
  tags: [{ name: 'Store' }, { name: 'Pet' }],
  paths: {
    '/pets': {
      get: {
        tags: ['Pet'],
        summary: 'List pets',
        parameters: [
          { name: 'limit', in: 'query', schema: { type: 'integer' } },
          { name: 'x-trace-id', in: 'header', required: false }
        ],
        responses: { '200': { description: 'ok' } }
      },
      post: {
        tags: ['Pet'],
        operationId: 'createPet',
        requestBody: {
          content: {
            'application/json': {
              schema: {
                type: 'object',
                properties: {
                  name: { type: 'string' },
                  age: { type: 'integer' },
                  vaccinated: { type: 'boolean' }
                },
                required: ['name']
              }
            }
          }
        },
        responses: { '201': { description: 'created' } }
      }
    },
    '/stores/{storeId}/orders': {
      get: {
        tags: ['Store'],
        summary: 'List store orders',
        parameters: [
          { name: 'storeId', in: 'path', required: true, schema: { type: 'string' } },
          { name: 'status', in: 'query', schema: { type: 'string', example: 'open' } }
        ]
      }
    }
  }
};

describe('parseOpenApiToCollection', () => {
  it('imports OAS 3 JSON into a collection tree using the first server URL', () => {
    const result = parseOpenApiToCollection(JSON.stringify(spec));

    expect(result.collection.name).toBe('Petstore Admin');
    expect(result.baseUrl).toBe('https://api.example.com/v1');
    // 分组按 operation.tags[0],顺序遵循文档顶层 tags 定义(Store 在前)
    expect(result.folders.map((f) => f.name)).toEqual(['Store', 'Pet']);
    expect(result.requests.map((r) => r.name)).toEqual([
      'GET List pets',
      'POST createPet',
      'GET List store orders'
    ]);
    const petFolder = result.folders.find((f) => f.name === 'Pet')!;
    const storeFolder = result.folders.find((f) => f.name === 'Store')!;
    const byName = (n: string) => result.requests.find((r) => r.name === n)!;
    expect(byName('GET List pets').folderId).toBe(petFolder.id);
    expect(byName('POST createPet').folderId).toBe(petFolder.id);
    expect(byName('GET List store orders').folderId).toBe(storeFolder.id);
    expect(result.requests[0].spec.url).toBe('https://api.example.com/v1/pets');
    expect(result.requests[2].spec.url).toBe('https://api.example.com/v1/stores/{{storeId}}/orders');
  });

  it('maps query/header/path parameters and JSON request body examples', () => {
    const result = parseOpenApiToCollection(JSON.stringify(spec), { baseUrl: '{{baseUrl}}' });
    const listPets = result.requests.find((r) => r.name === 'GET List pets')!;
    const createPet = result.requests.find((r) => r.name === 'POST createPet')!;
    const listOrders = result.requests.find((r) => r.name === 'GET List store orders')!;

    expect(listPets.spec.url).toBe('{{baseUrl}}/pets');
    expect(listPets.spec.queryParams).toEqual([
      { key: 'limit', value: '', enabled: true, description: '' }
    ]);
    expect(listPets.spec.headers).toEqual([
      { key: 'x-trace-id', value: '', enabled: true, description: '' }
    ]);
    expect(listOrders.spec.url).toContain('{{storeId}}');
    expect(listOrders.spec.queryParams[0]).toMatchObject({ key: 'status', value: 'open' });
    expect(createPet.spec.bodyType).toBe('json');
    expect(JSON.parse(createPet.spec.body)).toEqual({ name: 'string', age: 0, vaccinated: false });
  });

  it('splits slash-delimited tags into a nested folder tree', () => {
    const nested = {
      openapi: '3.0.3',
      info: { title: 'Nested' },
      paths: {
        '/a': { get: { tags: ['广告平台/Source'], summary: 'a' } },
        '/b': { get: { tags: ['广告平台/Sink'], summary: 'b' } },
        '/c': { get: { summary: 'c' } }
      }
    };
    const result = parseOpenApiToCollection(JSON.stringify(nested));

    const byName = (n: string) => result.folders.find((f) => f.name === n)!;
    const platform = byName('广告平台');
    const source = byName('Source');
    const sink = byName('Sink');
    // 顶层 folder 无 parent,子 folder 指向父
    expect(platform.parentId).toBeNull();
    expect(source.parentId).toBe(platform.id);
    expect(sink.parentId).toBe(platform.id);
    // 同名父层 “广告平台” 只建一次
    expect(result.folders.filter((f) => f.name === '广告平台')).toHaveLength(1);
    // 请求挂在最深层 folder 上
    expect(result.requests.find((r) => r.name === 'GET a')!.folderId).toBe(source.id);
    expect(result.requests.find((r) => r.name === 'GET b')!.folderId).toBe(sink.id);
    // 无 tag 的接口归到默认分组
    const uncategorized = result.folders.find((f) => f.name === '未分组')!;
    expect(uncategorized).toBeTruthy();
    expect(result.requests.find((r) => r.name === 'GET c')!.folderId).toBe(uncategorized.id);
  });

  it('imports components.schemas into a pinned 数据模型 folder', () => {
    const withSchemas = {
      openapi: '3.0.3',
      info: { title: 'WithSchemas' },
      paths: {
        '/ping': { get: { tags: ['Ops'], summary: 'ping' } }
      },
      components: {
        schemas: {
          消息事件: { type: 'object', properties: { id: { type: 'string' } }, required: ['id'] },
          any: {},
          null: { type: 'null' }
        }
      }
    };
    const result = parseOpenApiToCollection(JSON.stringify(withSchemas));

    const modelFolder = result.folders.find((f) => f.name === '数据模型')!;
    expect(modelFolder).toBeTruthy();
    // 置顶:orderIndex 小于任何接口 folder
    const opsFolder = result.folders.find((f) => f.name === 'Ops')!;
    expect(modelFolder.orderIndex).toBeLessThan(opsFolder.orderIndex);
    expect(modelFolder.parentId).toBeNull();

    const models = result.requests.filter((r) => r.spec.metaKind === 'schema');
    expect(models.map((m) => m.name).sort()).toEqual(['any', 'null', '消息事件']);
    for (const m of models) {
      expect(m.folderId).toBe(modelFolder.id);
      expect(m.spec.bodyType).toBe('json');
      expect(m.sourceKey).toBe(`schema:${m.name}`);
    }
    // body 是原始结构定义(保留 type/properties),不是展开的示例值
    const evt = models.find((m) => m.name === '消息事件')!;
    expect(JSON.parse(evt.spec.body)).toEqual({
      type: 'object',
      properties: { id: { type: 'string' } },
      required: ['id']
    });
  });

  it('does not create 数据模型 folder when there are no schemas', () => {
    const noSchemas = {
      openapi: '3.0.3',
      info: { title: 'NoSchemas' },
      paths: { '/ping': { get: { tags: ['Ops'], summary: 'ping' } } }
    };
    const result = parseOpenApiToCollection(JSON.stringify(noSchemas));
    expect(result.folders.find((f) => f.name === '数据模型')).toBeUndefined();
    expect(result.requests.every((r) => r.spec.metaKind !== 'schema')).toBe(true);
  });

  it('rejects unsupported or empty documents', () => {
    expect(() => parseOpenApiToCollection('{}')).toThrow('只支持 OpenAPI 3.x JSON');
    expect(() => parseOpenApiToCollection(JSON.stringify({ openapi: '3.0.0', info: {}, paths: {} })))
      .toThrow('没有可导入的 HTTP operation');
  });
});
