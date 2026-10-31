// Makes cargo rebuild when a migration is added; sqlx::migrate! embeds the directory.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
