import { invoke } from "@tauri-apps/api/core";

/** Raw native values never escape this boundary, including on decode failure. */
export async function invokeDecoded<T>(
  command: string,
  args: Record<string, unknown>,
  decode: (value: unknown) => T | null,
  invalidResponseCode: string,
): Promise<T> {
  const value = await invoke<unknown>(command, args);
  const decoded = decode(value);
  if (decoded === null) throw new Error(invalidResponseCode);
  return decoded;
}
