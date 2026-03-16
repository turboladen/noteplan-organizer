import { invoke } from "@tauri-apps/api/core";
import type {
  ContentBlock,
  FilingSuggestion,
  FilingTarget,
  McpStatus,
  Report,
} from "../types/api";

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
  return invoke<void>("open_noteplan_url", { url });
}

export async function systemDump(path: string): Promise<string> {
  return invoke<string>("system_dump", { path });
}

export async function exportAssessmentContext(
  path: string,
  guideTitle?: string,
): Promise<string> {
  return invoke<string>("export_assessment_context", {
    path,
    guide_title: guideTitle ?? null,
  });
}

export async function startWatching(path: string): Promise<void> {
  return invoke<void>("start_watching", { path });
}

export async function stopWatching(): Promise<void> {
  return invoke<void>("stop_watching");
}

export async function isWatching(): Promise<boolean> {
  return invoke<boolean>("is_watching");
}

export async function getGitRev(): Promise<string> {
  return invoke<string>("get_git_rev");
}

// ---------------------------------------------------------------------------
// Content blocks (filing assistant)
// ---------------------------------------------------------------------------

export async function getContentBlocks(
  notePath: string,
): Promise<ContentBlock[]> {
  return invoke<ContentBlock[]>("get_content_blocks", { note_path: notePath });
}

export async function getFilingTargets(path: string): Promise<FilingTarget[]> {
  return invoke<FilingTarget[]>("get_filing_targets", { path });
}

export async function getFilingSuggestions(
  basePath: string,
  notePath: string,
): Promise<FilingSuggestion[]> {
  return invoke<FilingSuggestion[]>("get_filing_suggestions", {
    base_path: basePath,
    note_path: notePath,
  });
}

// ---------------------------------------------------------------------------
// MCP integration commands
// ---------------------------------------------------------------------------

export async function mcpConnect(): Promise<string> {
  return invoke<string>("mcp_connect");
}

export async function mcpDisconnect(): Promise<void> {
  return invoke<void>("mcp_disconnect");
}

export async function mcpStatus(): Promise<McpStatus> {
  return invoke<McpStatus>("mcp_status");
}

export async function mcpCallTool(
  toolName: string,
  args: Record<string, unknown>,
): Promise<string> {
  return invoke<string>("mcp_call_tool", {
    tool_name: toolName,
    arguments: args,
  });
}
