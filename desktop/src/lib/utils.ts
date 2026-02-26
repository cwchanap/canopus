/**
 * Extract a human-readable message from any thrown value.
 * Tauri commands return `CommandError` shaped objects `{ code, message }`
 * rather than `Error` instances, so `String(e)` would produce `[object Object]`.
 */
export function extractErrorMessage(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === "object" && e !== null) {
    const obj = e as Record<string, unknown>;
    if (typeof obj.message === "string") {
      return typeof obj.code === "string"
        ? `[${obj.code}] ${obj.message}`
        : obj.message;
    }
  }
  return String(e);
}
