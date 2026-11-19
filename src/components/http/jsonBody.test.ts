import { describe, expect, it } from 'vitest';
import { formatJsonBody, validateJsonBody } from './jsonBody';

describe('validateJsonBody', () => {
  it('rejects an empty JSON request body', () => {
    expect(validateJsonBody('  \n')).toEqual({
      ok: false,
      error: 'JSON 请求体不能为空'
    });
  });

  it('keeps the parser detail for invalid JSON', () => {
    const result = validateJsonBody('{"name":"AT HTTP"');

    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error).toContain('JSON 格式错误：');
  });

  it('accepts valid JSON values', () => {
    expect(validateJsonBody('{"enabled":true}')).toEqual({ ok: true });
  });
});

describe('formatJsonBody', () => {
  it('formats valid JSON with two-space indentation', () => {
    expect(formatJsonBody('{"name":"AT HTTP","items":[1,2]}')).toEqual({
      ok: true,
      value: '{\n  "name": "AT HTTP",\n  "items": [\n    1,\n    2\n  ]\n}'
    });
  });

  it('does not return replacement text for invalid JSON', () => {
    const result = formatJsonBody('{"name":}');

    expect(result.ok).toBe(false);
  });
});
