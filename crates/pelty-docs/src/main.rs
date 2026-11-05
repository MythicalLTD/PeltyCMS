//! `pelty-docs [OUT_DIR]`, writes the developer docs (HTML, Markdown, JSON)
//! to `OUT_DIR` (default `target/docs`). Run by `cargo xtask docs` (or `make docs`).
use pelty_docs::{doc, extract, pages, routes};
use std::path::PathBuf;

fn main() {
    let out = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| extract::repo().join("target/docs"));
    match pelty_docs::generate(&out) {
        Ok((u, d)) => println!("pelty-docs: wrote {u} user pages to {0} and {d} developer pages to {0}/developers", out.display()),
        Err(e) => {
            eprintln!("pelty-docs: {e}");
            std::process::exit(1);
        }
    }
    let _ = (doc::esc, pages::GUIDE_SRC, routes::API_PREFIX);
}
