/**
 * Converts a camelCase string to snake_case.
 */
function camelToSnake(str: string): string {
  return str.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`);
}

/**
 * Recursively converts all keys in an object from camelCase to snake_case.
 * Arrays are traversed, primitives are returned as-is.
 */
export function camelToSnakeKeys<T>(obj: unknown): T {
  if (obj === null || obj === undefined) {
    return obj as T;
  }
  if (Array.isArray(obj)) {
    return obj.map((item) => camelToSnakeKeys(item)) as T;
  }
  if (typeof obj === 'object') {
    const result: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(obj as Record<string, unknown>)) {
      const snakeKey = camelToSnake(key);
      result[snakeKey] = camelToSnakeKeys(value);
    }
    return result as T;
  }
  return obj as T;
}
