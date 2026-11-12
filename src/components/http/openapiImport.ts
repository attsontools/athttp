import type { HttpMethod, HttpRequestSpec } from './types';
import { makeEmptySpec } from './types';
import { SCHEMA_FOLDER_NAME, schemaSourceKey } from './schemaItem';

const METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'] as const;

interface OpenApiParameter {
  name?: string;
  in?: string;
  description?: string;
  schema?: JsonSchema;
  example?: unknown;
}

interface JsonSchema {
  type?: string;
  properties?: Record<string, JsonSchema>;
  items?: JsonSchema;
  required?: string[];
  example?: unknown;
  default?: unknown;
  enum?: unknown[];
}

interface OpenApiOperation {
  operationId?: string;
  summary?: string;
  tags?: string[];
  parameters?: OpenApiParameter[];
  requestBody?: {
    content?: Record<string, { schema?: JsonSchema; example?: unknown }>;
  };
}

interface OpenApiDocument {
  openapi?: string;
  info?: { title?: string };
  servers?: Array<{ url?: string }>;
  tags?: Array<{ name?: string }>;
  paths?: Record<string, Record<string, OpenApiOperation | unknown>>;
  components?: { schemas?: Record<string, JsonSchema> };
}

// 无 tag 接口归入的默认分组名
const UNCATEGORIZED_FOLDER = '未分组';

export interface ImportedCollection {
  id: string;
  name: string;
  orderIndex: number;
}

export interface ImportedFolder {
  id: string;
  collectionId: string;
  parentId: string | null;
  name: string;
  orderIndex: number;
}

export interface ImportedRequest {
  id: string;
  collectionId: string;
  folderId: string | null;
  name: string;
  method: HttpMethod;
  spec: HttpRequestSpec;
  orderIndex: number;
  sourceKey: string;
}

export interface ImportedOpenApiCollection {
  collection: ImportedCollection;
  folders: ImportedFolder[];
  requests: ImportedRequest[];
  baseUrl: string;
}

export interface OpenApiImportOptions {
  baseUrl?: string;
  collectionName?: string;
}

let nextId = 0;

function makeId(prefix: string): string {
  nextId += 1;
  return `${prefix}-${Date.now().toString(36)}-${nextId.toString(36)}`;
}

export function parseOpenApiToCollection(input: string, options: OpenApiImportOptions = {}): ImportedOpenApiCollection {
  let doc: OpenApiDocument;
  try {
    doc = JSON.parse(input) as OpenApiDocument;
  } catch {
    throw new Error('OpenAPI JSON 解析失败');
  }

  if (!doc.openapi?.startsWith('3.')) {
    throw new Error('只支持 OpenAPI 3.x JSON');
  }

  const collectionId = makeId('col');
  const baseUrl = normalizeBaseUrl(options.baseUrl ?? doc.servers?.find((s) => s.url)?.url ?? '');
  const collection: ImportedCollection = {
    id: collectionId,
    name: (options.collectionName || doc.info?.title || 'OpenAPI Collection').trim(),
    orderIndex: 0
  };
  const folders: ImportedFolder[] = [];
  const requests: ImportedRequest[] = [];
  // key 为 folder 的完整层级路径(如 "广告平台/Source"),用于去重与父层复用
  const folderByPath = new Map<string, ImportedFolder>();

  // 先按文档顶层 tags 声明顺序预建 folder,让分组顺序与 Swagger 展示一致
  for (const tag of doc.tags ?? []) {
    if (tag?.name) ensureFolderPath(tag.name, collectionId, folders, folderByPath);
  }

  for (const [path, pathItem] of Object.entries(doc.paths ?? {})) {
    if (!pathItem || typeof pathItem !== 'object') continue;

    for (const [methodKey, operationRaw] of Object.entries(pathItem as Record<string, unknown>)) {
      const method = methodKey.toUpperCase();
      if (!isHttpMethod(method) || !operationRaw || typeof operationRaw !== 'object') continue;
      const operation = operationRaw as OpenApiOperation;
      const tag = operation.tags?.find((t) => t && t.trim()) ?? UNCATEGORIZED_FOLDER;
      const folder = ensureFolderPath(tag, collectionId, folders, folderByPath);
      const spec = operationToSpec(method, baseUrl, path, operation);
      requests.push({
        id: makeId('req'),
        collectionId,
        folderId: folder?.id ?? null,
        name: `${method} ${operation.summary || operation.operationId || path}`.slice(0, 80),
        method,
        spec,
        orderIndex: requests.length,
        sourceKey: `${method} ${path}`
      });
    }
  }

  // ---- 数据模型(components.schemas)----
  const schemas = doc.components?.schemas ?? {};
  const schemaNames = Object.keys(schemas);
  if (schemaNames.length > 0) {
    // 「数据模型」置顶 folder:orderIndex = -1 保证排在所有接口 folder(0..n)之前
    const modelFolder: ImportedFolder = {
      id: makeId('fld'),
      collectionId,
      parentId: null,
      name: SCHEMA_FOLDER_NAME,
      orderIndex: -1
    };
    folders.push(modelFolder);
    for (const name of schemaNames) {
      const spec = makeEmptySpec();
      spec.method = 'GET';
      spec.bodyType = 'json';
      spec.body = JSON.stringify(schemas[name], null, 2);
      spec.metaKind = 'schema';
      requests.push({
        id: makeId('req'),
        collectionId,
        folderId: modelFolder.id,
        name,
        method: 'GET',
        spec,
        orderIndex: requests.length,
        sourceKey: schemaSourceKey(name)
      });
    }
  }

  if (requests.length === 0) {
    throw new Error('没有可导入的 HTTP operation');
  }

  return { collection, folders, requests, baseUrl };
}

function operationToSpec(method: HttpMethod, baseUrl: string, path: string, operation: OpenApiOperation): HttpRequestSpec {
  const spec = makeEmptySpec();
  spec.method = method;
  spec.url = joinUrl(baseUrl, replacePathParams(path));
  spec.queryParams = [];
  spec.headers = [];

  for (const parameter of operation.parameters ?? []) {
    if (!parameter.name) continue;
    const value = exampleValue(parameter.example ?? parameter.schema?.example ?? parameter.schema?.default);
    const row = {
      key: parameter.name,
      value,
      enabled: true,
      description: parameter.description ?? ''
    };
    if (parameter.in === 'query') spec.queryParams.push(row);
    else if (parameter.in === 'header') spec.headers.push(row);
  }

  const jsonBody = operation.requestBody?.content?.['application/json'];
  if (jsonBody) {
    spec.bodyType = 'json';
    spec.body = JSON.stringify(
      jsonBody.example ?? schemaExample(jsonBody.schema),
      null,
      2
    );
  }

  return spec;
}

function isHttpMethod(value: string): value is HttpMethod {
  return (METHODS as readonly string[]).includes(value);
}

// 按 "/" 把 tag 拆成层级,逐层建立或复用嵌套 folder,返回最深一层 folder。
// 同名同父路径的 folder 只建一次(以完整路径为 key 去重)。
function ensureFolderPath(
  tag: string,
  collectionId: string,
  folders: ImportedFolder[],
  folderByPath: Map<string, ImportedFolder>
): ImportedFolder | null {
  const segments = tag.split('/').map((s) => s.trim()).filter(Boolean);
  if (segments.length === 0) return null;

  let parentId: string | null = null;
  let current: ImportedFolder | null = null;
  let pathKey = '';
  for (const name of segments) {
    pathKey = pathKey ? `${pathKey}/${name}` : name;
    let folder = folderByPath.get(pathKey);
    if (!folder) {
      folder = {
        id: makeId('fld'),
        collectionId,
        parentId,
        name,
        orderIndex: folders.length
      };
      folderByPath.set(pathKey, folder);
      folders.push(folder);
    }
    parentId = folder.id;
    current = folder;
  }
  return current;
}

function replacePathParams(path: string): string {
  return path.replace(/\{([^}]+)\}/g, '{{$1}}');
}

function normalizeBaseUrl(url: string): string {
  return url.trim().replace(/\/+$/, '');
}

function joinUrl(baseUrl: string, path: string): string {
  const normalizedPath = path.startsWith('/') ? path : `/${path}`;
  return baseUrl ? `${baseUrl}${normalizedPath}` : normalizedPath;
}

function exampleValue(value: unknown): string {
  if (value === undefined || value === null) return '';
  return String(value);
}

function schemaExample(schema?: JsonSchema): unknown {
  if (!schema) return {};
  if (schema.example !== undefined) return schema.example;
  if (schema.default !== undefined) return schema.default;
  if (schema.enum?.length) return schema.enum[0];
  if (schema.type === 'array') return [schemaExample(schema.items)];
  if (schema.type === 'integer' || schema.type === 'number') return 0;
  if (schema.type === 'boolean') return false;
  if (schema.type === 'object' || schema.properties) {
    const out: Record<string, unknown> = {};
    for (const [key, child] of Object.entries(schema.properties ?? {})) {
      out[key] = schemaExample(child);
    }
    return out;
  }
  return 'string';
}
