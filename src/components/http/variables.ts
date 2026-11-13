import type { HttpRequestSpec, KV, MultipartField, SseSpec, WsSpec } from './types';

export interface VarContext {
  active: Map<string, string>;
  global: Map<string, string>;
}

const VAR_RE = /\{\{\s*([A-Za-z_][A-Za-z0-9_-]*)\s*\}\}/g;

export const COMMON_ENV_VAR_OPTIONS = [
  { label: 'baseUrl', value: 'baseUrl' },
  { label: 'token', value: 'token' },
  { label: 'apiKey', value: 'apiKey' }
] as const;

export function makeVarContext(
  activeVars: Array<{ key: string; value: string; enabled: boolean }>,
  globalVars: Array<{ key: string; value: string; enabled: boolean }>
): VarContext {
  const active = new Map<string, string>();
  const global = new Map<string, string>();
  for (const v of globalVars) if (v.enabled && v.key) global.set(v.key, v.value);
  for (const v of activeVars) if (v.enabled && v.key) active.set(v.key, v.value);
  return { active, global };
}

export function lookupVar(name: string, ctx: VarContext): string | undefined {
  if (ctx.active.has(name)) return ctx.active.get(name);
  if (ctx.global.has(name)) return ctx.global.get(name);
  return undefined;
}

export function resolveVars(input: string, ctx: VarContext): string {
  if (!input) return input;
  return input.replace(VAR_RE, (raw, name) => {
    const found = lookupVar(name, ctx);
    return found !== undefined ? found : raw;
  });
}

export type RequestUrlKind = 'http' | 'sse' | 'ws';

export function resolveRequestUrl(input: string, ctx: VarContext, kind: RequestUrlKind): string {
  const unknownVars = collectUnknownVars(input, ctx);
  if (unknownVars.length > 0) {
    throw new Error(`变量未定义：${unknownVars.join(', ')}`);
  }
  const resolved = resolveVars(input, ctx);
  if (/^(?:https?|wss?):\/\//i.test(resolved)) return resolved;

  const baseUrl = lookupVar('baseUrl', ctx)?.trim();
  if (!baseUrl) throw new Error('相对 URL 需要配置有效的 baseUrl');
  let parsedBase: URL;
  try {
    parsedBase = new URL(baseUrl);
  } catch {
    throw new Error('相对 URL 需要配置有效的 baseUrl');
  }
  const allowedProtocols = kind === 'ws'
    ? new Set(['http:', 'https:', 'ws:', 'wss:'])
    : new Set(['http:', 'https:']);
  if (
    !allowedProtocols.has(parsedBase.protocol)
    || !parsedBase.host
    || /[?#]/.test(baseUrl)
    || !/^(?:https?|wss?):\/\//i.test(baseUrl)
  ) {
    throw new Error('相对 URL 需要配置有效的 baseUrl');
  }
  if (kind === 'ws') {
    if (parsedBase.protocol === 'https:') parsedBase.protocol = 'wss:';
    if (parsedBase.protocol === 'http:') parsedBase.protocol = 'ws:';
  }
  const effectiveBase = parsedBase.toString();
  return `${effectiveBase.replace(/\/+$/, '')}/${resolved.replace(/^\/+/, '')}`;
}

export function collectVars(input: string): string[] {
  const out: string[] = [];
  if (!input) return out;
  const seen = new Set<string>();
  let m: RegExpExecArray | null;
  const re = new RegExp(VAR_RE.source, 'g');
  while ((m = re.exec(input)) !== null) {
    if (!seen.has(m[1])) {
      seen.add(m[1]);
      out.push(m[1]);
    }
  }
  return out;
}

export function collectUnknownVars(input: string, ctx: VarContext): string[] {
  return collectVars(input).filter((name) => lookupVar(name, ctx) === undefined);
}

function resolveKvList(list: KV[], ctx: VarContext): KV[] {
  return list.map((kv) => ({
    ...kv,
    key: resolveVars(kv.key, ctx),
    value: resolveVars(kv.value, ctx)
  }));
}

function resolveMultipart(fields: MultipartField[], ctx: VarContext): MultipartField[] {
  return fields.map((field) => ({
    ...field,
    key: resolveVars(field.key, ctx),
    // 文件路径不做替换
    value: field.kind === 'file' ? field.value : resolveVars(field.value, ctx)
  }));
}

function resolveKvListPub(list: KV[] | undefined, ctx: VarContext): KV[] {
  if (!list) return [];
  return list.map((kv) => ({
    ...kv,
    key: resolveVars(kv.key, ctx),
    value: resolveVars(kv.value, ctx),
  }));
}

export function applyVarsToSseSpec(spec: SseSpec, ctx: VarContext): SseSpec {
  const auth = spec.auth ?? { type: 'none' as const };
  return {
    ...spec,
    url: resolveRequestUrl(spec.url ?? '', ctx, 'sse'),
    headers: resolveKvListPub(spec.headers, ctx),
    queryParams: resolveKvListPub(spec.queryParams, ctx),
    auth: {
      ...auth,
      bearerToken: auth.bearerToken ? resolveVars(auth.bearerToken, ctx) : undefined,
      basicUser: auth.basicUser ? resolveVars(auth.basicUser, ctx) : undefined,
      basicPass: auth.basicPass ? resolveVars(auth.basicPass, ctx) : undefined,
    },
    lastEventId: spec.lastEventId ? resolveVars(spec.lastEventId, ctx) : undefined,
  };
}

export function applyVarsToWsSpec(spec: WsSpec, ctx: VarContext): WsSpec {
  const auth = spec.auth ?? { type: 'none' as const };
  return {
    ...spec,
    url: resolveRequestUrl(spec.url ?? '', ctx, 'ws'),
    headers: resolveKvListPub(spec.headers, ctx),
    queryParams: resolveKvListPub(spec.queryParams, ctx),
    auth: {
      ...auth,
      bearerToken: auth.bearerToken ? resolveVars(auth.bearerToken, ctx) : undefined,
      basicUser: auth.basicUser ? resolveVars(auth.basicUser, ctx) : undefined,
      basicPass: auth.basicPass ? resolveVars(auth.basicPass, ctx) : undefined,
    },
  };
}

export function applyVarsToSpec(spec: HttpRequestSpec, ctx: VarContext): HttpRequestSpec {
  return {
    ...spec,
    url: resolveRequestUrl(spec.url, ctx, 'http'),
    headers: resolveKvList(spec.headers, ctx),
    queryParams: resolveKvList(spec.queryParams, ctx),
    body: spec.bodyType === 'multipart' ? spec.body : resolveVars(spec.body, ctx),
    multipartFields: resolveMultipart(spec.multipartFields, ctx),
    auth: {
      ...spec.auth,
      bearerToken: spec.auth.bearerToken ? resolveVars(spec.auth.bearerToken, ctx) : undefined,
      basicUser: spec.auth.basicUser ? resolveVars(spec.auth.basicUser, ctx) : undefined,
      basicPass: spec.auth.basicPass ? resolveVars(spec.auth.basicPass, ctx) : undefined
    }
  };
}
