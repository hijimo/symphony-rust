/**
 * Converts a camelCase string to snake_case.
 */
function camelToSnake(str: string): string {
  return str.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`);
}

/**
 * Converts a snake_case string to camelCase.
 */
function snakeToCamel(str: string): string {
  return str.replace(/_([a-z0-9])/g, (_, ch: string) => ch.toUpperCase());
}

/**
 * Recursively converts all keys in an object from snake_case to camelCase.
 * Keys already in camelCase are left untouched.
 */
export function snakeToCamelKeys<T>(obj: unknown): T {
  if (obj === null || obj === undefined) {
    return obj as T;
  }
  if (Array.isArray(obj)) {
    return obj.map((item) => snakeToCamelKeys(item)) as T;
  }
  if (typeof obj === 'object') {
    const result: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(obj as Record<string, unknown>)) {
      result[snakeToCamel(key)] = snakeToCamelKeys(value);
    }
    return result as T;
  }
  return obj as T;
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
