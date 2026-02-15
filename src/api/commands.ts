import { invoke } from "@tauri-apps/api/core";
import type { Report } from "../types/api";

export async function detectNotePlanPath(): Promise<string> {
  return invoke<string>("detect_noteplan_path");
}

export async function scanNotes(path: string): Promise<Report> {
  return invoke<Report>("scan", { path });
}

export async function getNoteContent(path: string): Promise<string> {
  return invoke<string>("get_note_content", { path });
}

export async function openNotePlanUrl(url: string): Promise<void> {
  return invoke("open_noteplan_url", { url });
}

export async function startWatching(path: string): Promise<void> {
  return invoke("start_watching", { path });
}

export async function stopWatching(): Promise<void> {
  return invoke("stop_watching");
}

export async function isWatching(): Promise<boolean> {
  return invoke<boolean>("is_watching");
}
