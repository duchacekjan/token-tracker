import { invoke } from "@tauri-apps/api/core";
import type {
  PricingStatus,
  SessionDetail,
  SessionSummary,
  Settings,
  SettingsView,
} from "./types";

export const listSessions = () => invoke<SessionSummary[]>("list_sessions");

export const getSession = (filePath: string) =>
  invoke<SessionDetail>("get_session", { filePath });

export const getSettings = () => invoke<SettingsView>("get_settings");

export const saveSettings = (settings: Settings) =>
  invoke<SettingsView>("save_settings", { settings });

export const refreshPricing = () => invoke<PricingStatus>("refresh_pricing");
