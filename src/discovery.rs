//! Tool discovery — SearchIndex for AI-agent-driven tool search.

use crate::manifest::Manifest;
use emporium_core::tool;
use serde::Serialize;

/// Index of all extension tools, supporting fuzzy text search.
///
/// Built from loaded extension manifests. Updated when extensions
/// fire `tools-changed` events.
pub struct SearchIndex {
    extensions: Vec<ExtensionEntry>,
}

struct ExtensionEntry {
    id: String,
    name: String,
    overview: String,
    topics: Vec<String>,
    tools: Vec<ToolEntry>,
}

struct ToolEntry {
    id: String,
    name: String,
    description: String,
    schema: serde_json::Value,
    examples: Vec<serde_json::Value>,
    primary: bool,
}

/// A single search result with full tool metadata.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub extension_id: String,
    pub extension_name: String,
    pub tool_id: String,
    pub tool_name: String,
    pub tool_description: String,
    pub schema: serde_json::Value,
    pub examples: Vec<serde_json::Value>,
    pub primary: bool,
    /// Relevance score (higher is better). Not stable across versions.
    pub score: u32,
}

impl SearchIndex {
    /// Build from loaded extension manifests.
    ///
    /// Each tuple is `(extension_id, manifest)`. The extension_id should
    /// match `manifest.id` but is accepted separately for flexibility.
    pub fn from_manifests(manifests: &[(String, Manifest)]) -> Self {
        let extensions = manifests
            .iter()
            .map(|(ext_id, manifest)| {
                let tools = manifest
                    .tools
                    .iter()
                    .map(|t| ToolEntry {
                        id: t.id.clone(),
                        name: t.name.clone(),
                        description: t.description.clone(),
                        schema: t.schema.clone(),
                        examples: t.examples.clone(),
                        primary: t.primary,
                    })
                    .collect();
                ExtensionEntry {
                    id: ext_id.clone(),
                    name: manifest.name.clone(),
                    overview: manifest.overview.clone().unwrap_or_default(),
                    topics: manifest.topics.clone(),
                    tools,
                }
            })
            .collect();
        SearchIndex { extensions }
    }

    /// Update a single extension's tools from runtime `list_tools()`.
    ///
    /// Called when a `tools-changed` event fires. Replaces the tool list
    /// for the given extension, preserving the extension-level metadata.
    pub fn update_extension(&mut self, ext_id: &str, tools: Vec<tool::Info>) {
        if let Some(ext) = self.extensions.iter_mut().find(|e| e.id == ext_id) {
            ext.tools = tools
                .into_iter()
                .map(|t| ToolEntry {
                    id: t.id,
                    name: t.name,
                    description: t.description,
                    schema: t.schema,
                    examples: t.examples,
                    primary: false, // runtime tools are non-primary by default
                })
                .collect();
        }
    }

    /// Fuzzy search across all extensions' tools.
    ///
    /// Tokenizes the query into words, scores each tool by word matches
    /// against tool name (+3), description (+2), extension overview (+1),
    /// and topics (+1). Returns top-N results sorted by score descending.
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let query_words = tokenize(query);
        if query_words.is_empty() {
            return Vec::new();
        }

        let mut scored: Vec<SearchResult> = Vec::new();

        for ext in &self.extensions {
            // Pre-compute extension-level tokens
            let overview_lower = ext.overview.to_lowercase();
            let topics_lower: Vec<String> = ext.topics.iter().map(|t| t.to_lowercase()).collect();

            for tool in &ext.tools {
                let tool_name_lower = tool.name.to_lowercase();
                let tool_desc_lower = tool.description.to_lowercase();
                let tool_id_lower = tool.id.to_lowercase();

                let mut score: u32 = 0;

                for word in &query_words {
                    // Tool name match: +3
                    if tool_name_lower.contains(word.as_str()) || tool_id_lower.contains(word.as_str()) {
                        score += 3;
                    }

                    // Tool description match: +2
                    if tool_desc_lower.contains(word.as_str()) {
                        score += 2;
                    }

                    // Extension overview match: +1
                    if overview_lower.contains(word.as_str()) {
                        score += 1;
                    }

                    // Topics match: +1
                    if topics_lower.iter().any(|t| t.contains(word.as_str())) {
                        score += 1;
                    }
                }

                if score > 0 {
                    scored.push(SearchResult {
                        extension_id: ext.id.clone(),
                        extension_name: ext.name.clone(),
                        tool_id: tool.id.clone(),
                        tool_name: tool.name.clone(),
                        tool_description: tool.description.clone(),
                        schema: tool.schema.clone(),
                        examples: tool.examples.clone(),
                        primary: tool.primary,
                        score,
                    });
                }
            }
        }

        scored.sort_by_key(|b| std::cmp::Reverse(b.score));
        scored.truncate(limit);
        scored
    }

    /// Return the number of indexed tools across all extensions.
    pub fn tool_count(&self) -> usize {
        self.extensions.iter().map(|e| e.tools.len()).sum()
    }

    /// Return the number of indexed extensions.
    pub fn extension_count(&self) -> usize {
        self.extensions.len()
    }
}

/// Tokenize a query string into lowercase words, stripping punctuation.
fn tokenize(query: &str) -> Vec<String> {
    query
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_'))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// The JSON schema for the `search_tools` meta-tool, suitable for
/// inclusion in LLM tool definitions.
pub fn search_tools_schema() -> serde_json::Value {
    serde_json::json!({
        "name": "search_tools",
        "description": "Search for tools across all installed extensions. Use this when you need a capability not available in your current tools.",
        "input_schema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What you're looking for, e.g. 'RSI technical indicator' or 'forex exchange rate' or 'SQL query'"
                }
            },
            "required": ["query"]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use emporium_types::ManifestTool;
    use serde_json::json;

    /// Build a test manifest with the given tools and metadata.
    fn test_manifest(overview: Option<&str>, topics: &[&str], tools: Vec<ManifestTool>) -> Manifest {
        Manifest {
            id: "test-ext".to_string(),
            name: "Test Extension".to_string(),
            version: "0.1.0".to_string(),
            description: "A test extension".to_string(),
            overview: overview.map(String::from),
            topics: topics.iter().map(|s| s.to_string()).collect(),
            author: "Test".to_string(),
            company: None,
            license: "MIT".to_string(),
            homepage: None,
            repository: None,
            keywords: vec![],
            categories: vec![],
            features: vec![],
            capabilities: Default::default(),
            tools,
            data_sources: vec![],
            config_schema: json!({}),
            world: None,
        }
    }

    fn make_tool(id: &str, name: &str, desc: &str, primary: bool) -> ManifestTool {
        ManifestTool {
            id: id.to_string(),
            name: name.to_string(),
            description: desc.to_string(),
            schema: json!({"type": "object"}),
            cacheable: false,
            primary,
            activity: None,
            examples: vec![],
        }
    }

    #[test]
    fn empty_index_returns_no_results() {
        let index = SearchIndex::from_manifests(&[]);
        assert!(index.search("anything", 10).is_empty());
        assert_eq!(index.tool_count(), 0);
        assert_eq!(index.extension_count(), 0);
    }

    #[test]
    fn empty_query_returns_no_results() {
        let manifest = test_manifest(Some("overview"), &[], vec![make_tool("t", "Tool", "desc", false)]);
        let index = SearchIndex::from_manifests(&[("ext".to_string(), manifest)]);
        assert!(index.search("", 10).is_empty());
        assert!(index.search("   ", 10).is_empty());
    }

    #[test]
    fn search_matches_tool_name() {
        let manifest = test_manifest(None, &[], vec![
            make_tool("rsi", "RSI (Relative Strength Index)", "Momentum indicator", false),
            make_tool("sma", "SMA (Simple Moving Average)", "Trend indicator", false),
        ]);
        let index = SearchIndex::from_manifests(&[("alphav".to_string(), manifest)]);
        let results = index.search("RSI", 10);
        assert!(!results.is_empty());
        assert_eq!(results[0].tool_id, "rsi");
    }

    #[test]
    fn search_matches_tool_description() {
        let manifest = test_manifest(None, &[], vec![
            make_tool("t1", "Tool A", "Fetches stock prices from the API", false),
            make_tool("t2", "Tool B", "Sends email notifications", false),
        ]);
        let index = SearchIndex::from_manifests(&[("ext".to_string(), manifest)]);
        let results = index.search("stock prices", 10);
        assert!(!results.is_empty());
        assert_eq!(results[0].tool_id, "t1");
    }

    #[test]
    fn search_matches_extension_overview() {
        let manifest = test_manifest(Some("Financial data, forex rates, and crypto prices"), &[], vec![
            make_tool("fetch", "Fetch", "Generic data fetch", false),
        ]);
        let index = SearchIndex::from_manifests(&[("fin".to_string(), manifest)]);
        let results = index.search("forex", 10);
        assert!(!results.is_empty());
        assert_eq!(results[0].extension_id, "fin");
    }

    #[test]
    fn search_matches_topics() {
        let manifest = test_manifest(None, &["finance", "equities"], vec![make_tool(
            "t", "Tool", "A tool", false,
        )]);
        let index = SearchIndex::from_manifests(&[("ext".to_string(), manifest)]);
        let results = index.search("finance", 10);
        assert!(!results.is_empty());
    }

    #[test]
    fn search_respects_limit() {
        let tools: Vec<ManifestTool> = (0..20)
            .map(|i| make_tool(&format!("t{i}"), &format!("Tool {i}"), "matching description", false))
            .collect();
        let manifest = test_manifest(None, &[], tools);
        let index = SearchIndex::from_manifests(&[("ext".to_string(), manifest)]);
        let results = index.search("matching", 5);
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn search_sorts_by_score_descending() {
        let manifest = test_manifest(Some("stock market data"), &["stocks"], vec![
            // This tool matches on name + description + overview + topics = high score
            make_tool("stock_quote", "Stock Quote", "Get stock price data", true),
            // This tool only matches on overview + topics = lower score
            make_tool("forex", "Forex Rate", "Get forex exchange rate", false),
        ]);
        let index = SearchIndex::from_manifests(&[("ext".to_string(), manifest)]);
        let results = index.search("stock", 10);
        assert!(results.len() >= 2);
        assert!(results[0].score >= results[1].score);
        assert_eq!(results[0].tool_id, "stock_quote");
    }

    #[test]
    fn tool_count_sums_across_extensions() {
        let m1 = test_manifest(None, &[], vec![
            make_tool("a", "A", "a", false),
            make_tool("b", "B", "b", false),
        ]);
        let m2 = test_manifest(None, &[], vec![make_tool("c", "C", "c", false)]);
        let index = SearchIndex::from_manifests(&[("ext1".to_string(), m1), ("ext2".to_string(), m2)]);
        assert_eq!(index.tool_count(), 3);
        assert_eq!(index.extension_count(), 2);
    }

    #[test]
    fn update_extension_replaces_tools() {
        let manifest = test_manifest(None, &[], vec![make_tool("old", "Old", "old tool", false)]);
        let mut index = SearchIndex::from_manifests(&[("ext".to_string(), manifest)]);
        assert_eq!(index.tool_count(), 1);

        let new_tools = vec![
            tool::Info {
                id: "new1".to_string(),
                name: "New One".to_string(),
                description: "first new tool".to_string(),
                schema: json!({}),
                cacheable: false,
                activity: None,
                examples: vec![],
            },
            tool::Info {
                id: "new2".to_string(),
                name: "New Two".to_string(),
                description: "second new tool".to_string(),
                schema: json!({}),
                cacheable: false,
                activity: None,
                examples: vec![],
            },
        ];
        index.update_extension("ext", new_tools);
        assert_eq!(index.tool_count(), 2);
        assert!(index.search("old", 10).is_empty());
        assert!(!index.search("new", 10).is_empty());
    }

    #[test]
    fn search_tools_schema_has_required_fields() {
        let schema = search_tools_schema();
        assert_eq!(schema["name"], "search_tools");
        assert!(schema["input_schema"]["properties"]["query"].is_object());
    }

    #[test]
    fn tokenize_handles_punctuation_and_whitespace() {
        let tokens = tokenize("RSI, momentum (indicator)");
        assert_eq!(tokens, vec!["rsi", "momentum", "indicator"]);
    }

    #[test]
    fn tokenize_preserves_hyphens() {
        let tokens = tokenize("technical-analysis stocks");
        assert_eq!(tokens, vec!["technical-analysis", "stocks"]);
    }
}
