export type JsonValidationResult =
  | { ok: true }
  | { ok: false; error: string };

export type JsonFormatResult =
  | { ok: true; value: string }
  | { ok: false; error: string };

export function validateJsonBody(input: string): JsonValidationResult {
  if (!input.trim()) return { ok: false, error: 'JSON 请求体不能为空' };

  try {
    JSON.parse(input);
    return { ok: true };
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    return { ok: false, error: `JSON 格式错误：${detail}` };
  }
}

export function formatJsonBody(input: string): JsonFormatResult {
  const validation = validateJsonBody(input);
  if (!validation.ok) return validation;

  return {
    ok: true,
    value: JSON.stringify(JSON.parse(input), null, 2)
  };
}
