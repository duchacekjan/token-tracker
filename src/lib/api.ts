import { invoke } from "@tauri-apps/api/core";
import type {
  PricingStatus,
  SessionDetail,
  SessionKey,
  SessionSummary,
  Settings,
  SettingsView,
  Source,
} from "./types";
import { mockApi } from "./mock";

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const listSessions = (): Promise<SessionSummary[]> =>
  inTauri ? invoke<SessionSummary[]>("list_sessions") : mockApi.listSessions();

export const getSession = (source: Source, id: string): Promise<SessionDetail> =>
  inTauri ? invoke<SessionDetail>("get_session", { source, id }) : mockApi.getSession(source, id);

export const getSettings = (): Promise<SettingsView> =>
  inTauri ? invoke<SettingsView>("get_settings") : mockApi.getSettings();

export const saveSettings = (settings: Settings): Promise<SettingsView> =>
  inTauri ? invoke<SettingsView>("save_settings", { settings }) : mockApi.saveSettings(settings);

export const getDbWarning = (): Promise<string | null> =>
  inTauri ? invoke<string | null>("get_db_warning") : mockApi.getDbWarning();

export const refreshPricing = (): Promise<PricingStatus> =>
  inTauri ? invoke<PricingStatus>("refresh_pricing") : mockApi.refreshPricing();

export const getSessionsDetail = (keys: SessionKey[]): Promise<SessionDetail[]> =>
  inTauri ? invoke<SessionDetail[]>("get_sessions_detail", { keys }) : mockApi.getSessionsDetail(keys);

/** Tauri: native save dialog; returns saved path or null if cancelled. Browser: Blob download; returns defaultName. */
export async function saveCsv(defaultName: string, contents: string): Promise<string | null> {
  if (inTauri) return invoke<string | null>("save_csv", { defaultName, contents });
  const url = URL.createObjectURL(new Blob([contents], { type: "text/csv;charset=utf-8" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = defaultName;
  a.style.display = "none";
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
  return defaultName;
}
