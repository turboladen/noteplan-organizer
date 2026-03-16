use serde::Serialize;

/// Results from the parsing backend benchmark.
#[derive(Serialize)]
pub struct BenchmarkResult {
    /// Rust file parser: wall-clock milliseconds to scan all notes
    pub rust_scan_ms: u64,
    /// Number of notes found by the Rust parser
    pub rust_note_count: usize,
    /// MCP list: wall-clock milliseconds to list all notes (None if MCP not connected)
    pub mcp_list_ms: Option<u64>,
    /// MCP list: number of notes reported (None if MCP not connected)
    pub mcp_note_count: Option<usize>,
    /// MCP single-note retrieval: average ms per note over a sample (None if MCP not connected)
    pub mcp_avg_get_ms: Option<f64>,
    /// Number of notes sampled for the MCP get timing
    pub mcp_sample_size: Option<usize>,
}
