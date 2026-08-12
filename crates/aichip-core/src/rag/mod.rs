//! Semantic retrieval over a space's documents.
//!
//! Everything is local: chunks live in Postgres, embeddings are ONNX
//! inference on this machine (fastembed), and ranking is brute-force cosine
//! in Rust. No model API is ever called — the compliance rule this app is
//! built around — and no vector extension is required, because a document
//! space is thousands of chunks and brute force ranks that in milliseconds.
//!
//! The model itself is fetched once from HuggingFace and cached under
//! `~/.aichip/models`: an artifact download, the same class as a cargo
//! dependency, carrying no user content in either direction.

pub mod chunk;
pub mod embed;
pub mod index;
pub mod retrieve;
