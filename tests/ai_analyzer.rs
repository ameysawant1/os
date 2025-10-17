//! Unit tests for the AI kernel components
//! These tests run on the host system and test pure Rust logic.

/// Basic AI Text Analyzer for semantic processing (copied from main.rs for testing)
struct TextAnalyzer {
    // Simple keyword-based categorization
    tech_keywords: &'static [&'static str],
    creative_keywords: &'static [&'static str],
    data_keywords: &'static [&'static str],
}

impl TextAnalyzer {
    const fn new() -> Self {
        TextAnalyzer {
            tech_keywords: &["code", "program", "kernel", "memory", "cpu", "system", "os", "rust", "compile", "algorithm", "software", "hardware", "computer", "programming", "development"],
            creative_keywords: &["design", "art", "music", "write", "writing", "create", "story", "stories", "image", "video", "creative", "aesthetic", "beautiful", "interface", "ui", "ux"],
            data_keywords: &["data", "analyze", "chart", "graph", "statistics", "database", "query", "search", "analytics", "visualization", "pattern", "trend", "model"],
        }
    }

    fn analyze_text(&self, text: &str) -> TextCategory {
        let mut tech_score = 0;
        let mut creative_score = 0;
        let mut data_score = 0;

        // Simple case-insensitive keyword matching using byte arrays
        let text_bytes = text.as_bytes();
        let mut text_lower = [0u8; 256];
        for (i, &byte) in text_bytes.iter().enumerate().take(255) {
            text_lower[i] = byte.to_ascii_lowercase();
        }

        // Count keyword matches (case-insensitive)
        for &keyword in self.tech_keywords.iter() {
            if self.contains_keyword(&text_lower, keyword) {
                tech_score += 1;
            }
        }

        for &keyword in self.creative_keywords.iter() {
            if self.contains_keyword(&text_lower, keyword) {
                creative_score += 1;
            }
        }

        for &keyword in self.data_keywords.iter() {
            if self.contains_keyword(&text_lower, keyword) {
                data_score += 1;
            }
        }

        // Determine category based on highest score
        if tech_score >= creative_score && tech_score >= data_score && tech_score > 0 {
            TextCategory::Technical
        } else if creative_score >= data_score && creative_score > 0 {
            TextCategory::Creative
        } else if data_score > 0 {
            TextCategory::Data
        } else {
            // No keywords matched - default to Data
            TextCategory::Data
        }
    }

    fn contains_keyword(&self, text_lower: &[u8; 256], keyword: &str) -> bool {
        let keyword_bytes = keyword.as_bytes();
        let keyword_len = keyword_bytes.len();
        if keyword_len == 0 {
            return false;
        }

        let text_len = text_lower.iter().position(|&x| x == 0).unwrap_or(text_lower.len());

        for i in 0..=(text_len.saturating_sub(keyword_len)) {
            let mut matches = true;
            for j in 0..keyword_len {
                let keyword_char = keyword_bytes[j].to_ascii_lowercase();
                if text_lower[i + j] != keyword_char {
                    matches = false;
                    break;
                }
            }
            if matches {
                return true;
            }
        }
        false
    }

    fn extract_features(&self, text: &str) -> TextFeatures {
        let char_count = text.chars().count();
        let word_count = text.split_whitespace().count();
        let has_numbers = text.chars().any(|c| c.is_numeric());
        let has_punctuation = text.chars().any(|c| !c.is_alphanumeric() && !c.is_whitespace());

        TextFeatures {
            char_count,
            word_count,
            has_numbers,
            has_punctuation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TextCategory {
    Technical,
    Creative,
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TextFeatures {
    char_count: usize,
    word_count: usize,
    has_numbers: bool,
    has_punctuation: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_analyzer_creation() {
        let analyzer = TextAnalyzer::new();
        assert!(!analyzer.tech_keywords.is_empty());
        assert!(!analyzer.creative_keywords.is_empty());
        assert!(!analyzer.data_keywords.is_empty());
    }

    #[test]
    fn test_technical_text_analysis() {
        let analyzer = TextAnalyzer::new();

        let tech_texts = vec![
            "This kernel is written in Rust programming language",
            "The memory management system uses paging and virtual memory",
            "CPU scheduling algorithms optimize system performance",
            "Operating system kernel development requires careful design",
        ];

        for text in tech_texts {
            let category = analyzer.analyze_text(text);
            assert_eq!(category, TextCategory::Technical,
                      "Text '{}' should be classified as Technical", text);
        }
    }

    #[test]
    fn test_creative_text_analysis() {
        let analyzer = TextAnalyzer::new();

        let creative_texts = vec![
            "Creating beautiful user interfaces requires design skills",
            "Digital art and music production tools are essential",
            "Writing compelling stories engages readers emotionally",
            "Video editing software enables creative expression",
        ];

        for text in creative_texts {
            let category = analyzer.analyze_text(text);
            assert_eq!(category, TextCategory::Creative,
                      "Text '{}' should be classified as Creative", text);
        }
    }

    #[test]
    fn test_data_text_analysis() {
        let analyzer = TextAnalyzer::new();

        let data_texts = vec![
            "Data analysis shows interesting patterns in user behavior",
            "Statistical models help predict future trends",
            "Database queries retrieve information efficiently",
            "Chart visualization makes data more understandable",
        ];

        for text in data_texts {
            let category = analyzer.analyze_text(text);
            assert_eq!(category, TextCategory::Data,
                      "Text '{}' should be classified as Data", text);
        }
    }

    #[test]
    fn test_keyword_detection() {
        let analyzer = TextAnalyzer::new();

        // Test technical keywords
        assert_eq!(analyzer.analyze_text("rust code compilation"), TextCategory::Technical);
        assert_eq!(analyzer.analyze_text("kernel memory management"), TextCategory::Technical);
        assert_eq!(analyzer.analyze_text("cpu system optimization"), TextCategory::Technical);

        // Test creative keywords
        assert_eq!(analyzer.analyze_text("design art creation"), TextCategory::Creative);
        assert_eq!(analyzer.analyze_text("music video production"), TextCategory::Creative);
        assert_eq!(analyzer.analyze_text("story writing skills"), TextCategory::Creative);

        // Test data keywords
        assert_eq!(analyzer.analyze_text("data analysis statistics"), TextCategory::Data);
        assert_eq!(analyzer.analyze_text("database query search"), TextCategory::Data);
        assert_eq!(analyzer.analyze_text("chart graph visualization"), TextCategory::Data);
    }

    #[test]
    fn test_case_insensitive_matching() {
        let analyzer = TextAnalyzer::new();

        // Test that keyword matching is case-insensitive
        assert_eq!(analyzer.analyze_text("RUST programming"), TextCategory::Technical);
        assert_eq!(analyzer.analyze_text("Design ART"), TextCategory::Creative);
        assert_eq!(analyzer.analyze_text("DATA analysis"), TextCategory::Data);
    }

    #[test]
    fn test_feature_extraction() {
        let analyzer = TextAnalyzer::new();

        let text = "Hello, world! This is a test with 123 numbers.";
        let features = analyzer.extract_features(text);

        assert_eq!(features.char_count, text.chars().count());
        assert_eq!(features.word_count, 9); // "Hello,", "world!", "This", "is", "a", "test", "with", "123", "numbers."
        assert!(features.has_numbers);
        assert!(features.has_punctuation);
    }

    #[test]
    fn test_empty_text() {
        let analyzer = TextAnalyzer::new();

        let features = analyzer.extract_features("");
        assert_eq!(features.char_count, 0);
        assert_eq!(features.word_count, 0);
        assert!(!features.has_numbers);
        assert!(!features.has_punctuation);
    }

    #[test]
    fn test_text_without_keywords_defaults_to_data() {
        let analyzer = TextAnalyzer::new();

        // Text with no keywords should default to Data category (lowest priority)
        let category = analyzer.analyze_text("This is some generic text without specific keywords");
        assert_eq!(category, TextCategory::Data);
    }

    #[test]
    fn test_mixed_keywords_priority() {
        let analyzer = TextAnalyzer::new();

        // Text with mixed keywords - should prioritize highest scoring category
        let tech_text = analyzer.analyze_text("code design data analysis");
        assert_eq!(tech_text, TextCategory::Technical); // tech=1, creative=1, data=1 -> tech wins

        let creative_text = analyzer.analyze_text("design art data");
        assert_eq!(creative_text, TextCategory::Creative); // tech=0, creative=2, data=1 -> creative wins
    }
}