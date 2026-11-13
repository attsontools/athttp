import { describe, expect, it } from 'vitest';
import {
  applyVarsToSpec,
  applyVarsToSseSpec,
  applyVarsToWsSpec,
  COMMON_ENV_VAR_OPTIONS,
  collectUnknownVars,
  collectVars,
  makeVarContext,
  resolveRequestUrl,
  resolveVars
} from './variables';
import { makeEmptySpec } from './types';

const ctx = makeVarContext(
  [
    { key: 'baseUrl', value: 'https://api.prod', enabled: true },
    { key: 'token', value: 'abc', enabled: true },
    { key: 'disabled', value: 'x', enabled: false }
  ],
  [
    { key: 'baseUrl', value: 'https://api.global', enabled: true },
    { key: 'timeout', value: '5000', enabled: true }
  ]
);

describe('resolveVars', () => {
  it('replaces {{name}}', () => {
    expect(resolveVars('{{baseUrl}}/users', ctx)).toBe('https://api.prod/users');
  });

  it('active takes priority over global', () => {
    expect(resolveVars('{{baseUrl}}', ctx)).toBe('https://api.prod');
  });

  it('falls back to global when active has no key', () => {
    expect(resolveVars('t={{timeout}}', ctx)).toBe('t=5000');
  });

  it('keeps unknown vars literal', () => {
    expect(resolveVars('{{missing}}/x', ctx)).toBe('{{missing}}/x');
  });

  it('skips disabled entries', () => {
    expect(resolveVars('{{disabled}}', ctx)).toBe('{{disabled}}');
  });

  it('tolerates whitespace inside braces', () => {
    expect(resolveVars('{{ token }}', ctx)).toBe('abc');
  });

  it('replaces multiple occurrences', () => {
    expect(resolveVars('{{token}}-{{token}}', ctx)).toBe('abc-abc');
  });

  it('leaves single braces alone', () => {
    expect(resolveVars('{token}', ctx)).toBe('{token}');
  });

  it('returns empty string as-is', () => {
    expect(resolveVars('', ctx)).toBe('');
  });
});

describe('common environment variable suggestions', () => {
  it('offers the agreed names without supplying values', () => {
    expect(COMMON_ENV_VAR_OPTIONS).toEqual([
      { label: 'baseUrl', value: 'baseUrl' },
      { label: 'token', value: 'token' },
      { label: 'apiKey', value: 'apiKey' }
    ]);
  });
});

describe('resolveRequestUrl', () => {
  it('uses the active baseUrl for an HTTP relative path', () => {
    const input = '/message_center/publish';

    expect(resolveRequestUrl(input, ctx, 'http')).toBe('https://api.prod/message_center/publish');
    expect(input).toBe('/message_center/publish');
  });

  it('maps an HTTPS baseUrl to WSS for a WebSocket relative path', () => {
    expect(resolveRequestUrl('socket/events', ctx, 'ws')).toBe('wss://api.prod/socket/events');
  });

  it('rejects unresolved URL variables instead of treating them as a relative path', () => {
    expect(() => resolveRequestUrl('{{missing}}/users', ctx, 'http')).toThrow(
      '变量未定义：missing'
    );
  });

  it('rejects a relative path when baseUrl is not an absolute URL', () => {
    const invalidCtx = makeVarContext(
      [{ key: 'baseUrl', value: 'api.internal', enabled: true }],
      []
    );

    expect(() => resolveRequestUrl('/users', invalidCtx, 'http')).toThrow(
      '相对 URL 需要配置有效的 baseUrl'
    );
  });

  it.each([
    'https://api.example.com/v1?tenant=one',
    'https://api.example.com/v1#section',
    'https://api.example.com/v1?',
    'https://api.example.com/v1#'
  ])('rejects a baseUrl whose suffix cannot be joined safely: %s', (baseUrl) => {
    const invalidCtx = makeVarContext(
      [{ key: 'baseUrl', value: baseUrl, enabled: true }],
      []
    );

    expect(() => resolveRequestUrl('/users', invalidCtx, 'http')).toThrow(
      '相对 URL 需要配置有效的 baseUrl'
    );
  });
});

describe('stream URL resolution', () => {
  it('resolves an SSE relative URL with the effective baseUrl', () => {
    const spec = { url: '/events', headers: [], queryParams: [], auth: { type: 'none' as const }, verifySsl: true };

    expect(applyVarsToSseSpec(spec, ctx).url).toBe('https://api.prod/events');
  });

  it('resolves a WebSocket relative URL with the effective baseUrl', () => {
    const spec = {
      url: '/socket',
      headers: [],
      queryParams: [],
      auth: { type: 'none' as const },
      verifySsl: true,
      subprotocols: [],
      templates: []
    };

    expect(applyVarsToWsSpec(spec, ctx).url).toBe('wss://api.prod/socket');
  });
});

describe('collectVars / collectUnknownVars', () => {
  it('lists unique variable names', () => {
    expect(collectVars('{{a}}/{{b}}/{{a}}')).toEqual(['a', 'b']);
  });

  it('returns [] for input without vars', () => {
    expect(collectVars('no vars here')).toEqual([]);
  });

  it('reports undefined vars only', () => {
    expect(collectUnknownVars('{{baseUrl}}/{{nope}}', ctx)).toEqual(['nope']);
  });
});

describe('applyVarsToSpec', () => {
  it('replaces URL / headers / query / body / auth', () => {
    const spec = makeEmptySpec();
    spec.url = '{{baseUrl}}/u';
    spec.headers = [{ key: 'X-{{token}}', value: 'v-{{token}}', enabled: true }];
    spec.queryParams = [{ key: 'q', value: '{{token}}', enabled: true }];
    spec.bodyType = 'json';
    spec.body = '{"t":"{{token}}"}';
    spec.auth = { type: 'bearer', bearerToken: '{{token}}' };
    const out = applyVarsToSpec(spec, ctx);
    expect(out.url).toBe('https://api.prod/u');
    expect(out.headers[0].key).toBe('X-abc');
    expect(out.headers[0].value).toBe('v-abc');
    expect(out.queryParams[0].value).toBe('abc');
    expect(out.body).toBe('{"t":"abc"}');
    expect(out.auth.bearerToken).toBe('abc');
  });

  it('resolves an HTTP relative URL with the effective baseUrl', () => {
    const spec = makeEmptySpec();
    spec.url = '/message_center/publish';

    expect(applyVarsToSpec(spec, ctx).url).toBe('https://api.prod/message_center/publish');
    expect(spec.url).toBe('/message_center/publish');
  });

  it('does not replace multipart file paths', () => {
    const spec = makeEmptySpec();
    spec.bodyType = 'multipart';
    spec.multipartFields = [
      { key: 'file', kind: 'file', value: '/abs/{{token}}.png', enabled: true },
      { key: 'note', kind: 'text', value: 'hi {{token}}', enabled: true }
    ];
    const out = applyVarsToSpec(spec, ctx);
    expect(out.multipartFields[0].value).toBe('/abs/{{token}}.png');
    expect(out.multipartFields[1].value).toBe('hi abc');
  });

  it('does not mutate the input spec', () => {
    const spec = makeEmptySpec();
    spec.url = '{{baseUrl}}';
    applyVarsToSpec(spec, ctx);
    expect(spec.url).toBe('{{baseUrl}}');
  });
});
