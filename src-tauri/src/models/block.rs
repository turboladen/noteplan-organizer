use serde::Serialize;

/// The kind of content block extracted from a daily note.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum BlockKind {
    /// A headed section (## Heading + content until next same/higher-level heading)
    Heading,
    /// A group of consecutive task lines not under a heading
    TaskGroup,
    /// Free-text paragraph(s) not under a heading
    Paragraph,
}

/// A discrete content block from a daily note — the unit of filing.
/// Each block is a candidate for being moved/copied to a project or domain note.
#[derive(Debug, Clone, Serialize)]
pub struct ContentBlock {
    pub kind: BlockKind,
    /// 1-based start line in the source note
    pub start_line: usize,
    /// 1-based end line (inclusive) in the source note
    pub end_line: usize,
    /// The raw text of this block (lines joined with \n)
    pub raw_text: String,
    /// For Heading blocks, the heading text (without ## prefix)
    pub heading: Option<String>,
    /// Heading level (1-6) for Heading blocks, None for others
    pub heading_level: Option<u8>,
    /// Tags found in this block (#tag)
    pub tags: Vec<String>,
    /// @mentions found in this block
    pub mentions: Vec<String>,
    /// [[wiki links]] found in this block
    pub wiki_links: Vec<String>,
}
