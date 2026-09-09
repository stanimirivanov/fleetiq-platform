//! Rebuilds the crate when its embedded SQL migrations change.

fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
