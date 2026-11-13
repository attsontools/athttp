import { describe, it, expect } from 'vitest';
import { isSchemaSpec, SCHEMA_FOLDER_NAME, schemaSourceKey } from './schemaItem';

describe('isSchemaSpec', () => {
  it('is true when metaKind is schema', () => {
    expect(isSchemaSpec({ metaKind: 'schema' })).toBe(true);
  });
  it('is false when metaKind is absent', () => {
    expect(isSchemaSpec({})).toBe(false);
  });
  it('is false for other metaKind values', () => {
    expect(isSchemaSpec({ metaKind: 'other' })).toBe(false);
  });
});

describe('schemaSourceKey', () => {
  it('prefixes the schema name', () => {
    expect(schemaSourceKey('消息事件')).toBe('schema:消息事件');
  });
});

describe('SCHEMA_FOLDER_NAME', () => {
  it('is the top pinned folder name', () => {
    expect(SCHEMA_FOLDER_NAME).toBe('数据模型');
  });
});
