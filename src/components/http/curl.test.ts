import { describe, expect, it } from 'vitest';
import { parseCurl, toCurl, tokenize } from './curl';
import { makeEmptySpec } from './types';

describe('tokenize', () => {
  it('handles double quotes', () => {
    expect(tokenize('curl "a b" c')).toEqual(['curl', 'a b', 'c']);
  });

  it('handles single quotes', () => {
    expect(tokenize("curl 'a b' c")).toEqual(['curl', 'a b', 'c']);
  });

  it('handles backslash line continuations', () => {
    expect(tokenize('curl \\\n  -X GET \\\n  http://x')).toEqual(['curl', '-X', 'GET', 'http://x']);
  });

  it('handles escaped double quotes inside double quotes', () => {
    expect(tokenize('curl "a\\"b"')).toEqual(['curl', 'a"b']);
  });
});

describe('parseCurl', () => {
  it('parses simple GET', () => {
    const r = parseCurl('curl https://x/y');
    expect('spec' in r).toBe(true);
    if ('spec' in r) {
      expect(r.spec.method).toBe('GET');
      expect(r.spec.url).toBe('https://x/y');
    }
  });

  it('parses -X POST with -d as text', () => {
    const r = parseCurl("curl -X POST -d 'hello' https://x");
    if ('error' in r) throw new Error(r.error);
    expect(r.spec.method).toBe('POST');
    expect(r.spec.bodyType).toBe('text');
    expect(r.spec.body).toBe('hello');
  });

  it('parses -d with json content-type as json', () => {
    const r = parseCurl(
      `curl -X POST -H 'Content-Type: application/json' -d '{"a":1}' https://x`
    );
    if ('error' in r) throw new Error(r.error);
    expect(r.spec.bodyType).toBe('json');
    expect(r.spec.body).toBe('{"a":1}');
  });

  it('parses multiple -H flags', () => {
    const r = parseCurl(`curl -H 'A: 1' -H 'B: 2' https://x`);
    if ('error' in r) throw new Error(r.error);
    expect(r.spec.headers).toHaveLength(2);
    expect(r.spec.headers[0].key).toBe('A');
    expect(r.spec.headers[1].key).toBe('B');
  });

  it('parses -F with @file as multipart file', () => {
    const r = parseCurl(`curl -F 'avatar=@/tmp/x.png' -F 'note=hi' https://x`);
    if ('error' in r) throw new Error(r.error);
    expect(r.spec.bodyType).toBe('multipart');
    expect(r.spec.multipartFields[0]).toMatchObject({ key: 'avatar', kind: 'file', value: '/tmp/x.png' });
    expect(r.spec.multipartFields[1]).toMatchObject({ key: 'note', kind: 'text', value: 'hi' });
  });

  it('parses -u as basic auth', () => {
    const r = parseCurl(`curl -u user:pw https://x`);
    if ('error' in r) throw new Error(r.error);
    expect(r.spec.auth.type).toBe('basic');
    expect(r.spec.auth.basicUser).toBe('user');
    expect(r.spec.auth.basicPass).toBe('pw');
  });

  it('parses --data-urlencode as form', () => {
    const r = parseCurl(`curl --data-urlencode 'a=1' --data-urlencode 'b=hi world' https://x`);
    if ('error' in r) throw new Error(r.error);
    expect(r.spec.bodyType).toBe('form');
    expect(r.spec.body).toBe('a=1&b=hi%20world');
  });

  it('handles -L and -k', () => {
    const r = parseCurl(`curl -L -k https://x`);
    if ('error' in r) throw new Error(r.error);
    expect(r.spec.followRedirects).toBe(true);
    expect(r.spec.verifySsl).toBe(false);
  });

  it('warns on unsupported --cookie-jar', () => {
    const r = parseCurl(`curl --cookie-jar /tmp/j https://x`);
    if ('error' in r) throw new Error(r.error);
    expect(r.warnings.length).toBeGreaterThan(0);
  });

  it('errors when URL missing', () => {
    const r = parseCurl(`curl -X POST -H 'A: 1'`);
    expect('error' in r).toBe(true);
  });

  it('accepts bare url without `curl` prefix', () => {
    const r = parseCurl('https://example.com');
    if ('error' in r) throw new Error(r.error);
    expect(r.spec.url).toBe('https://example.com');
  });
});

describe('toCurl', () => {
  it('emits method + url', () => {
    const spec = makeEmptySpec();
    spec.url = 'https://x/y';
    const out = toCurl(spec, null);
    expect(out).toContain('curl');
    expect(out).toContain('-X GET');
    expect(out).toContain('https://x/y');
  });

  it('emits headers', () => {
    const spec = makeEmptySpec();
    spec.url = 'https://x';
    spec.headers = [{ key: 'A', value: '1', enabled: true }];
    const out = toCurl(spec, null);
    expect(out).toContain(`-H 'A: 1'`);
  });

  it('emits bearer auth as Authorization header', () => {
    const spec = makeEmptySpec();
    spec.url = 'https://x';
    spec.auth = { type: 'bearer', bearerToken: 't0k' };
    const out = toCurl(spec, null);
    expect(out).toContain('Authorization: Bearer t0k');
  });

  it('emits basic auth as -u', () => {
    const spec = makeEmptySpec();
    spec.url = 'https://x';
    spec.auth = { type: 'basic', basicUser: 'u', basicPass: 'p' };
    const out = toCurl(spec, null);
    expect(out).toContain('-u');
    expect(out).toContain(`u:p`);
  });

  it('emits multipart -F entries', () => {
    const spec = makeEmptySpec();
    spec.url = 'https://x';
    spec.bodyType = 'multipart';
    spec.multipartFields = [
      { key: 'f', kind: 'file', value: '/tmp/x.png', enabled: true },
      { key: 'n', kind: 'text', value: 'hi', enabled: true }
    ];
    const out = toCurl(spec, null);
    expect(out).toContain('f=@/tmp/x.png');
    expect(out).toContain('n=hi');
  });

  it('round-trips a basic POST', () => {
    const spec = makeEmptySpec();
    spec.method = 'POST';
    spec.url = 'https://x/y';
    spec.headers = [{ key: 'Content-Type', value: 'application/json', enabled: true }];
    spec.bodyType = 'json';
    spec.body = '{"a":1}';
    const curl = toCurl(spec, null);
    const parsed = parseCurl(curl);
    if ('error' in parsed) throw new Error(parsed.error);
    expect(parsed.spec.method).toBe('POST');
    expect(parsed.spec.url).toBe('https://x/y');
    expect(parsed.spec.bodyType).toBe('json');
    expect(parsed.spec.body).toBe('{"a":1}');
  });
});
