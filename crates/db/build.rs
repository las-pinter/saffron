//! Rebuilds the crate when a migration changes, so `sqlx::migrate!` embeds it.

fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
