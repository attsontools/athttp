// 集合里「数据模型」置顶分组名
export const SCHEMA_FOLDER_NAME = '数据模型';

// 判别一个条目是否为数据模型(schema)而非可发送请求
export function isSchemaSpec(spec: { metaKind?: string }): boolean {
  return spec.metaKind === 'schema';
}

// schema 条目的 sourceKey 前缀,与请求的 "METHOD path" 区分,避免同步时误匹配
export function schemaSourceKey(name: string): string {
  return `schema:${name}`;
}
