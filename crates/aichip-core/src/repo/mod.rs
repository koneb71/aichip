//! The project's own code, indexed so a run does not start cold.
//!
//! Everything here is derived from the repository and never authored: the
//! Brain is the paragraph a person writes, this is the map read out of the
//! files. Both reach a run, and when they disagree the code wins — which is
//! why every rendering names the branch and commit it was read from.
//!
//! Local, like the rest: enumeration is `git ls-files`, embedding is ONNX
//! inference on this machine through `rag::embed`, and ranking is cosine in
//! Rust. No model API is called and no vector extension is required.

pub mod chunk;
pub mod enumerate;
pub mod imports;
pub mod index;
pub mod rank;
pub mod symbols;
