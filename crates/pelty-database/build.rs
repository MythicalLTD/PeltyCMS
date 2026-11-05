// Rebuild when migrations change (sqlx::migrate! embeds them).
fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
