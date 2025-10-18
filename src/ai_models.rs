//! AI Model Infrastructure
//!
//! Initial implementation with TF-IDF for text analysis.
//! Prepares foundation for future quantized models and ggml-based LLM runtimes.

use core::collections::BTreeMap;
#[cfg(feature = "alloc")]
use alloc::string::String;
#[cfg(feature = "alloc")]
use alloc::vec::Vec;

/// Simple TF-IDF vectorizer for text analysis
pub struct TfidfVectorizer {
    vocabulary: BTreeMap<String, usize>,
    idf_scores: Vec<f32>,
    max_features: usize,
}

impl TfidfVectorizer {
    /// Create a new TF-IDF vectorizer
    pub fn new(max_features: usize) -> Self {
        TfidfVectorizer {
            vocabulary: BTreeMap::new(),
            idf_scores: Vec::new(),
            max_features,
        }
    }

    /// Fit the vectorizer on a corpus of documents
    pub fn fit(&mut self, documents: &[&str]) {
        // Build vocabulary
        let mut term_document_freq = BTreeMap::new();

        for doc in documents {
            let tokens = self.tokenize(doc);
            let mut seen_terms = BTreeMap::new();

            for token in tokens {
                *seen_terms.entry(token.clone()).or_insert(0) += 1;
            }

            for (term, _) in seen_terms {
                *term_document_freq.entry(term).or_insert(0) += 1;
            }
        }

        // Build vocabulary with most frequent terms
        let mut term_freq: Vec<_> = term_document_freq.into_iter().collect();
        term_freq.sort_by(|a, b| b.1.cmp(&a.1));

        for (term, _) in term_freq.into_iter().take(self.max_features) {
            let idx = self.vocabulary.len();
            self.vocabulary.insert(term, idx);
        }

        // Calculate IDF scores
        let num_docs = documents.len() as f32;
        self.idf_scores = vec![0.0; self.vocabulary.len()];

        for doc in documents {
            let tokens = self.tokenize(doc);
            let mut seen_terms = BTreeMap::new();

            for token in tokens {
                seen_terms.insert(token, true);
            }

            for (term, _) in seen_terms {
                if let Some(&idx) = self.vocabulary.get(&term) {
                    self.idf_scores[idx] += 1.0;
                }
            }
        }

        for score in &mut self.idf_scores {
            *score = (num_docs / *score).ln() + 1.0; // Add 1 for smoothing
        }
    }

    /// Transform a document to TF-IDF vector
    pub fn transform(&self, document: &str) -> Vec<f32> {
        let mut tf_vector = vec![0.0; self.vocabulary.len()];
        let tokens = self.tokenize(document);
        let total_tokens = tokens.len() as f32;

        // Calculate term frequencies
        for token in tokens {
            if let Some(&idx) = self.vocabulary.get(&token) {
                tf_vector[idx] += 1.0;
            }
        }

        // Convert to TF-IDF
        for i in 0..tf_vector.len() {
            if tf_vector[i] > 0.0 {
                tf_vector[i] = (tf_vector[i] / total_tokens) * self.idf_scores[i];
            }
        }

        tf_vector
    }

    /// Simple tokenization (split on whitespace and convert to lowercase)
    fn tokenize(&self, text: &str) -> Vec<String> {
        text.split_whitespace()
            .map(|word| word.to_lowercase())
            .collect()
    }

    /// Get vocabulary size
    pub fn vocabulary_size(&self) -> usize {
        self.vocabulary.len()
    }
}

/// Simple text classifier using TF-IDF features
pub struct TextClassifier {
    vectorizer: TfidfVectorizer,
    weights: Vec<f32>,
    bias: f32,
}

impl TextClassifier {
    /// Create a new text classifier
    pub fn new(max_features: usize) -> Self {
        TextClassifier {
            vectorizer: TfidfVectorizer::new(max_features),
            weights: Vec::new(),
            bias: 0.0,
        }
    }

    /// Train the classifier (simple perceptron-like training)
    pub fn train(&mut self, documents: &[&str], labels: &[f32], learning_rate: f32, epochs: usize) {
        self.vectorizer.fit(documents);
        let feature_count = self.vectorizer.vocabulary_size();

        self.weights = vec![0.0; feature_count];
        self.bias = 0.0;

        for _ in 0..epochs {
            for (doc, &label) in documents.iter().zip(labels.iter()) {
                let features = self.vectorizer.transform(doc);
                let prediction = self.predict_features(&features);
                let error = label - prediction;

                // Update weights
                for i in 0..feature_count {
                    self.weights[i] += learning_rate * error * features[i];
                }
                self.bias += learning_rate * error;
            }
        }
    }

    /// Predict class for a document
    pub fn predict(&self, document: &str) -> f32 {
        let features = self.vectorizer.transform(document);
        self.predict_features(&features)
    }

    /// Predict using feature vector
    fn predict_features(&self, features: &[f32]) -> f32 {
        let mut score = self.bias;
        for (i, &weight) in self.weights.iter().enumerate() {
            if i < features.len() {
                score += weight * features[i];
            }
        }
        // Simple sigmoid activation
        1.0 / (1.0 + (-score).exp())
    }
}

/// Model manager for versioned AI models
pub struct ModelManager {
    models: BTreeMap<String, Box<dyn AIModel>>,
}

impl ModelManager {
    pub fn new() -> Self {
        ModelManager {
            models: BTreeMap::new(),
        }
    }

    /// Register a model with a version
    pub fn register_model(&mut self, name: String, version: String, model: Box<dyn AIModel>) {
        let key = format!("{}:{}", name, version);
        self.models.insert(key, model);
    }

    /// Get a model by name and version
    pub fn get_model(&self, name: &str, version: &str) -> Option<&Box<dyn AIModel>> {
        let key = format!("{}:{}", name, version);
        self.models.get(&key)
    }

    /// List available models
    pub fn list_models(&self) -> Vec<String> {
        self.models.keys().cloned().collect()
    }
}

/// Trait for AI models
pub trait AIModel {
    /// Process input and return result
    fn process(&self, input: &str) -> String;
}

/// Example TF-IDF based text classifier model
pub struct TfidfClassifierModel {
    classifier: TextClassifier,
    categories: Vec<String>,
}

impl TfidfClassifierModel {
    pub fn new(classifier: TextClassifier, categories: Vec<String>) -> Self {
        TfidfClassifierModel {
            classifier,
            categories,
        }
    }
}

impl AIModel for TfidfClassifierModel {
    fn process(&self, input: &str) -> String {
        let score = self.classifier.predict(input);
        let category_idx = if score > 0.5 { 1 } else { 0 };
        self.categories.get(category_idx).unwrap_or(&"unknown".to_string()).clone()
    }
}

/// Initialize AI infrastructure
pub fn init() {
    // TODO: Initialize model manager and load default models
    // For now, this is a placeholder for future AI model loading
}

/// Test AI functionality
#[cfg(test)]
pub fn test_ai() {
    let documents = [
        "This is a technical document about programming",
        "This is a creative writing piece",
        "Machine learning algorithms are complex",
        "Art and design require creativity",
        "Data structures and algorithms",
        "Painting and sculpture techniques",
    ];

    let labels = [1.0, 0.0, 1.0, 0.0, 1.0, 0.0]; // 1 = technical, 0 = creative

    let mut classifier = TextClassifier::new(100);
    classifier.train(&documents, &labels, 0.1, 100);

    let test_doc = "Neural networks and deep learning";
    let prediction = classifier.predict(test_doc);

    assert!(prediction > 0.5, "Should classify as technical content");
}