import { invoke } from "@tauri-apps/api/core";
import type {
  PricingStatus,
  SessionDetail,
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

export const refreshPricing = (): Promise<PricingStatus> =>
  inTauri ? invoke<PricingStatus>("refresh_pricing") : mockApi.refreshPricing();
