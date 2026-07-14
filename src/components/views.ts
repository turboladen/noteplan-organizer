export type AppView =
  | "board"
  | "backlog"
  | "filing"
  | "findings"
  | "assessment";

export type McpUiState = "connecting" | "connected" | "offline";

export const ALL_VIEWS: AppView[] = [
  "board",
  "backlog",
  "filing",
  "findings",
  "assessment",
];
