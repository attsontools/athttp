import type { HttpMethod, HttpRequestSpec, KV, MultipartField } from './types';
import { makeEmptySpec } from './types';
import type { VarContext } from './variables';
import { resolveVars } from './variables';

export interface ParseCurlOk {
  spec: HttpRequestSpec;
  warnings: string[];
}
export interface ParseCurlErr {
  error: string;
}

/** Tokenise a shell-ish command line. Handles single/double quotes and backslash line continuations. */
export function tokenize(input: string): string[] {
  const merged = input.replace(/\\\r?\n/g, ' ');
  const out: string[] = [];
  let buf = '';
  let i = 0;
  let quote: '"' | "'" | null = null;
  const push = () => {
    if (buf.length > 0) {
      out.push(buf);
      buf = '';
    }
  };
  while (i < merged.length) {
    const ch = merged[i];
    if (quote) {
      if (ch === '\\' && quote === '"' && i + 1 < merged.length) {
        buf += merged[i + 1];
        i += 2;
        continue;
      }
      if (ch === quote) {
        quote = null;
        i++;
        continue;
      }
      buf += ch;
      i++;
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch as '"' | "'";
      i++;
      continue;
    }
    if (ch === '\\' && i + 1 < merged.length) {
      buf += merged[i + 1];
      i += 2;
      continue;
    }
    if (/\s/.test(ch)) {
      push();
      i++;
      continue;
    }
    buf += ch;
    i++;
  }
  push();
  return out;
}

const METHODS: HttpMethod[] = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'];

function isMethod(v: string): v is HttpMethod {
  return METHODS.includes(v.toUpperCase() as HttpMethod);
}

function splitHeader(raw: string): { key: string; value: string } | null {
  const idx = raw.indexOf(':');
  if (idx < 0) return null;
  return { key: raw.slice(0, idx).trim(), value: raw.slice(idx + 1).trim() };
}

export function parseCurl(input: string): ParseCurlOk | ParseCurlErr {
  const spec = makeEmptySpec();
  const warnings: string[] = [];
  let tokens: string[];
  try {
    tokens = tokenize(input.trim());
  } catch (e) {
    return { error: '无法解析 cURL：' + String(e) };
  }
  if (tokens.length === 0) return { error: '输入为空' };
  if (tokens[0].toLowerCase() === 'curl') tokens.shift();

  const rawData: string[] = [];
  const urlEncodedData: Array<{ key: string; value: string }> = [];
  const formData: MultipartField[] = [];
  let methodSet = false;
  let url = '';
  let contentTypeHeader: string | null = null;

  const take = (i: number): [string, number] => {
    if (i + 1 >= tokens.length) throw new Error(`${tokens[i]} 缺少参数`);
    return [tokens[i + 1], i + 2];
  };

  try {
    for (let i = 0; i < tokens.length; ) {
      const tok = tokens[i];
      if (tok === '-X' || tok === '--request') {
        const [v, j] = take(i);
        if (isMethod(v)) spec.method = v.toUpperCase() as HttpMethod;
        methodSet = true;
        i = j;
      } else if (tok === '-H' || tok === '--header') {
        const [v, j] = take(i);
        const kv = splitHeader(v);
        if (kv) {
          if (kv.key.toLowerCase() === 'content-type') contentTypeHeader = kv.value.toLowerCase();
          spec.headers.push({ key: kv.key, value: kv.value, enabled: true });
        }
        i = j;
      } else if (tok === '-d' || tok === '--data' || tok === '--data-raw' || tok === '--data-binary') {
        const [v, j] = take(i);
        rawData.push(v);
        if (!methodSet) spec.method = 'POST';
        i = j;
      } else if (tok === '--data-urlencode') {
        const [v, j] = take(i);
        const eq = v.indexOf('=');
        if (eq >= 0) urlEncodedData.push({ key: v.slice(0, eq), value: v.slice(eq + 1) });
        else urlEncodedData.push({ key: v, value: '' });
        if (!methodSet) spec.method = 'POST';
        i = j;
      } else if (tok === '-F' || tok === '--form') {
        const [v, j] = take(i);
        const eq = v.indexOf('=');
        if (eq >= 0) {
          const k = v.slice(0, eq);
          const val = v.slice(eq + 1);
          if (val.startsWith('@')) {
            formData.push({ key: k, kind: 'file', value: val.slice(1), enabled: true });
          } else {
            formData.push({ key: k, kind: 'text', value: val, enabled: true });
          }
        }
        if (!methodSet) spec.method = 'POST';
        i = j;
      } else if (tok === '-u' || tok === '--user') {
        const [v, j] = take(i);
        const colon = v.indexOf(':');
        spec.auth = {
          type: 'basic',
          basicUser: colon >= 0 ? v.slice(0, colon) : v,
          basicPass: colon >= 0 ? v.slice(colon + 1) : ''
        };
        i = j;
      } else if (tok === '--url') {
        const [v, j] = take(i);
        url = v;
        i = j;
      } else if (tok === '-L' || tok === '--location') {
        spec.followRedirects = true;
        i++;
      } else if (tok === '-k' || tok === '--insecure') {
        spec.verifySsl = false;
        i++;
      } else if (tok === '--compressed') {
        i++; // ignore
      } else if (tok === '-e' || tok === '--referer') {
        const [v, j] = take(i);
        spec.headers.push({ key: 'Referer', value: v, enabled: true });
        i = j;
      } else if (tok === '-A' || tok === '--user-agent') {
        const [v, j] = take(i);
        spec.headers.push({ key: 'User-Agent', value: v, enabled: true });
        i = j;
      } else if (tok === '-b' || tok === '--cookie') {
        const [v, j] = take(i);
        spec.headers.push({ key: 'Cookie', value: v, enabled: true });
        i = j;
      } else if (tok === '-T' || tok === '--upload-file' || tok === '--cookie-jar') {
        warnings.push(`不支持的选项已忽略：${tok}`);
        // consume the paired value if it looks like a value (not a flag)
        if (i + 1 < tokens.length && !tokens[i + 1].startsWith('-')) i += 2;
        else i++;
      } else if (tok.startsWith('-')) {
        warnings.push(`未知选项已忽略：${tok}`);
        if (i + 1 < tokens.length && !tokens[i + 1].startsWith('-')) i += 2;
        else i++;
      } else {
        // Positional URL
        url = tok;
        i++;
      }
    }
  } catch (e) {
    return { error: String((e as Error).message ?? e) };
  }

  if (!url) return { error: '缺少 URL' };
  spec.url = url;

  if (formData.length > 0) {
    spec.bodyType = 'multipart';
    spec.multipartFields = formData;
  } else if (urlEncodedData.length > 0) {
    spec.bodyType = 'form';
    spec.body = urlEncodedData
      .map((p) => `${encodeURIComponent(p.key)}=${encodeURIComponent(p.value)}`)
      .join('&');
  } else if (rawData.length > 0) {
    spec.body = rawData.join('&');
    if (contentTypeHeader && contentTypeHeader.includes('json')) spec.bodyType = 'json';
    else if (contentTypeHeader && contentTypeHeader.includes('x-www-form-urlencoded')) spec.bodyType = 'form';
    else spec.bodyType = 'text';
  }

  return { spec, warnings };
}

// ---- generation ----

function shQuote(s: string): string {
  if (s === '') return "''";
  if (/^[A-Za-z0-9_./:@%+=,-]+$/.test(s)) return s;
  return `'${s.replace(/'/g, "'\\''")}'`;
}

export function toCurl(spec: HttpRequestSpec, ctx: VarContext | null): string {
  const resolve = (v: string): string => (ctx ? resolveVars(v, ctx) : v);
  const parts: string[] = ['curl'];
  parts.push(`-X ${spec.method}`);

  const urlObj = tryUrlWithQuery(resolve(spec.url), spec.queryParams, ctx);
  parts.push(shQuote(urlObj));

  for (const h of spec.headers) {
    if (!h.enabled || !h.key) continue;
    parts.push(`-H ${shQuote(`${resolve(h.key)}: ${resolve(h.value)}`)}`);
  }

  if (spec.auth?.type === 'basic') {
    const u = resolve(spec.auth.basicUser ?? '');
    const p = resolve(spec.auth.basicPass ?? '');
    parts.push(`-u ${shQuote(`${u}:${p}`)}`);
  } else if (spec.auth?.type === 'bearer' && spec.auth.bearerToken) {
    parts.push(`-H ${shQuote(`Authorization: Bearer ${resolve(spec.auth.bearerToken)}`)}`);
  }

  if (spec.bodyType === 'json' && spec.body) {
    parts.push(`-H ${shQuote('Content-Type: application/json')}`);
    parts.push(`--data-raw ${shQuote(resolve(spec.body))}`);
  } else if (spec.bodyType === 'form' && spec.body) {
    for (const [k, v] of new URLSearchParams(spec.body)) {
      parts.push(`--data-urlencode ${shQuote(`${resolve(k)}=${resolve(v)}`)}`);
    }
  } else if (spec.bodyType === 'text' && spec.body) {
    parts.push(`--data-raw ${shQuote(resolve(spec.body))}`);
  } else if (spec.bodyType === 'multipart') {
    for (const f of spec.multipartFields) {
      if (!f.enabled || !f.key) continue;
      if (f.kind === 'file') parts.push(`-F ${shQuote(`${resolve(f.key)}=@${f.value}`)}`);
      else parts.push(`-F ${shQuote(`${resolve(f.key)}=${resolve(f.value)}`)}`);
    }
  }

  if (!spec.followRedirects) {
    // curl 默认不跟随重定向，无操作
  } else {
    parts.push('-L');
  }
  if (!spec.verifySsl) parts.push('-k');

  return parts.join(' \\\n  ');
}

function tryUrlWithQuery(base: string, qs: KV[], ctx: VarContext | null): string {
  const resolve = (v: string): string => (ctx ? resolveVars(v, ctx) : v);
  const active = qs.filter((q) => q.enabled && q.key);
  if (active.length === 0) return base;
  try {
    const u = new URL(base);
    for (const q of active) u.searchParams.append(resolve(q.key), resolve(q.value));
    return u.toString();
  } catch {
    const suffix = active
      .map((q) => `${encodeURIComponent(resolve(q.key))}=${encodeURIComponent(resolve(q.value))}`)
      .join('&');
    return base.includes('?') ? `${base}&${suffix}` : `${base}?${suffix}`;
  }
}
