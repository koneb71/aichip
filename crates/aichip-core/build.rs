//! Rebuild when a migration is added or changed.
//!
//! `sqlx::migrate!` embeds the migration set at *compile* time. Adding a new
//! `.sql` file does not touch any Rust source, so cargo sees nothing to do,
//! reuses the cached binary, and the new migration silently never runs — the
//! server boots healthy, reports no error, and the column you just added is
//! simply absent. That cost real debugging time on 0027; this makes it
//! impossible.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
