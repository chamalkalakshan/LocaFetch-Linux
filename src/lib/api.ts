import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

export interface FileResult {
  id: number;
  path: string;
  filename: string;
  extension: string | null;
  size: number | null;
  modified_time: number | null;
  is_dir: boolean;
}

export interface Settings {
  indexed_folders: string[];
  excluded_folders: string[];
  max_results: number;
  theme: "dark" | "light";
  launch_at_startup: boolean;
  start_minimized: boolean;
  minimize_to_tray: boolean;
  hotkey: string;
  reindex_interval_hours: number;
}

export interface IndexStatus {
  is_indexing: boolean;
  files_indexed: number;
  current_path: string;
  last_indexed_at: number | null;
  error: string | null;
}

export const searchFiles = (query: string, filter = ""): Promise<FileResult[]> =>
  invoke("search_files", { query, filter });

export const startIndexing = (): Promise<void> => invoke("start_indexing");

export const getIndexStatus = (): Promise<IndexStatus> =>
  invoke("get_index_status");

export const getFileCount = (): Promise<number> => invoke("get_file_count");

export const openFile = (path: string): Promise<void> =>
  invoke("open_file", { path });

export const revealInExplorer = (path: string): Promise<void> =>
  invoke("reveal_in_explorer", { path });

export const copyPath = (path: string): Promise<void> =>
  invoke("copy_path", { path });

export const getSettings = (): Promise<Settings> => invoke("get_settings");

export const saveSettings = (settings: Settings): Promise<void> =>
  invoke("save_settings", { settings });

export const hideWindow = (): Promise<void> => invoke("hide_window");

export const updateHotkey = (hotkey: string): Promise<void> =>
  invoke("update_hotkey", { hotkey });

export const getDrives = (): Promise<string[]> => invoke("get_drives");

export const openWithDialog = (path: string): Promise<void> =>
  invoke("open_with_dialog", { path });

export const pickFolder = async (): Promise<string | null> => {
  const result = await open({ directory: true, multiple: false });
  if (typeof result === "string") return result;
  return null;
};

export function formatSize(bytes: number | null): string {
  if (bytes === null) return "";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
}

export function formatDate(ts: number | null): string {
  if (!ts) return "";
  return new Date(ts * 1000).toLocaleDateString();
}
