#![no_std]
#![no_main]

use core::arch::asm;

/// Simple AI Text Analyzer for userland
struct TextAnalyzer {
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
        let text_bytes = text.as_bytes();
        if text_bytes.len() > 255 {
            return TextCategory::Data; // Default
        }

        let mut text_lower = [0u8; 256];
        for (i, &byte) in text_bytes.iter().enumerate().take(255) {
            text_lower[i] = byte.to_ascii_lowercase();
        }

        let mut tech_score = 0;
        let mut creative_score = 0;
        let mut data_score = 0;

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

        if tech_score >= creative_score && tech_score >= data_score && tech_score > 0 {
            TextCategory::Technical
        } else if creative_score >= data_score && creative_score > 0 {
            TextCategory::Creative
        } else if data_score > 0 {
            TextCategory::Data
        } else {
            TextCategory::Data
        }
    }

    fn contains_keyword(&self, text_lower: &[u8; 256], keyword: &str) -> bool {
        let keyword_bytes = keyword.as_bytes();
        let keyword_len = keyword_bytes.len();

        if keyword_len == 0 || keyword_len > 256 {
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
}

#[derive(Debug, Clone, Copy)]
enum TextCategory {
    Technical,
    Creative,
    Data,
}

#[no_mangle]
pub extern "C" fn _start() {
    let analyzer = TextAnalyzer::new();

    let sample_texts = [
        "This kernel is written in Rust programming language",
        "The memory management system uses paging and virtual memory",
        "Data analysis shows interesting patterns in user behavior",
        "Creating beautiful user interfaces requires design skills",
    ];

    for text in sample_texts.iter() {
        let category = analyzer.analyze_text(text);

        let category_str = match category {
            TextCategory::Technical => "TECHNICAL",
            TextCategory::Creative => "CREATIVE",
            TextCategory::Data => "DATA",
        };

        // Print using syscall
        syscall_write(b"Analyzing text: ");
        syscall_write(text.as_bytes());
        syscall_write(b" -> ");
        syscall_write(category_str.as_bytes());
        syscall_write(b"\n");
    }

    // Exit
    loop {}
}

fn syscall_write(buf: *const u8, count: usize) {
    unsafe {
        asm!(
            "mov rax, 0",      // syscall number for write
            "mov rdi, 1",      // fd = stdout
            "mov rsi, {}",     // buf
            "mov rdx, {}",     // count
            "int 0x80",        // syscall
            in(reg) buf,
            in(reg) count,
        );
    }
}