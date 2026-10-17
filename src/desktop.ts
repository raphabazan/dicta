import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Explicit development fixtures. This branch is removed from release builds.
export const isPreview =
  import.meta.env.DEV && new URLSearchParams(location.search).has("preview");
export async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (isPreview)
    return (await import("./preview")).call(command, args) as Promise<T>;
  return invoke<T>(command, args);
}
export async function on<T>(
  name: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  if (isPreview) return (await import("./preview")).on(name, handler);
  return listen<T>(name, (event) => handler(event.payload));
}
