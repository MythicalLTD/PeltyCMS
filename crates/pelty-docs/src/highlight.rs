//! Syntax highlighting at generation time (syntect with the extra grammars
//! from `two-face`). Output uses `hl-<scope atom>` classes; the colours
//! (GitHub dark) live in `templates/style.css`, so pages need no JavaScript
//! to look right.
use crate::doc::esc;
use std::sync::OnceLock;
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

fn set() -> &'static SyntaxSet {
    static S: OnceLock<SyntaxSet> = OnceLock::new();
    S.get_or_init(two_face::syntax::extra_newlines)
}

/// Grammar for a fence language, `None` for plain text.
fn syntax(lang: &str) -> Option<&'static SyntaxReference> {
    let s = set();
    let token = match lang {
        "rust" | "rs" => "rs",
        "json" | "jsonc" => "json",
        "toml" => "toml",
        "bash" | "sh" | "shell" | "console" => "sh",
        "ts" | "typescript" => "ts",
        "tsx" => "tsx",
        "js" | "javascript" | "mjs" => "js",
        "c" | "h" => "c",
        "go" => "go",
        "python" | "py" => "py",
        "html" => "html",
        "css" => "css",
        "sql" => "sql",
        "yaml" | "yml" => "yaml",
        "dockerfile" => "Dockerfile",
        "ini" | "env" | "dotenv" => "ini",
        "nginx" => "nginx",
        "md" | "markdown" => "md",
        // WIT reads well with the Rust grammar (`func`, records, comments).
        "wit" => "rs",
        _ => return None,
    };
    s.find_syntax_by_token(token).or_else(|| s.find_syntax_by_extension(token))
}

/// Human label for the code bar.
pub fn label(lang: &str) -> &str {
    match lang {
        "" | "text" => "Text",
        "rust" | "rs" => "Rust",
        "json" => "JSON",
        "toml" => "TOML",
        "bash" | "sh" | "shell" | "console" => "Shell",
        "ts" | "typescript" => "TypeScript",
        "tsx" => "TSX",
        "js" | "javascript" | "mjs" => "JavaScript",
        "c" => "C",
        "go" => "Go",
        "python" | "py" => "Python",
        "html" => "HTML",
        "css" => "CSS",
        "sql" => "SQL",
        "yaml" | "yml" => "YAML",
        "wit" => "WIT",
        "md" => "Markdown",
        "env" | "dotenv" => ".env",
        "nginx" => "nginx",
        "dockerfile" => "Dockerfile",
        l => l,
    }
}

/// Highlighted HTML for `code` (escaped plain text when the language is unknown).
pub fn html(lang: &str, code: &str) -> String {
    let Some(sx) = syntax(lang) else { return esc(code) };
    let mut g = ClassedHTMLGenerator::new_with_class_style(sx, set(), ClassStyle::SpacedPrefixed { prefix: "hl-" });
    for line in LinesWithEndings::from(code) {
        if g.parse_html_for_line_which_includes_newline(line).is_err() {
            return esc(code);
        }
    }
    g.finalize()
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_documented_language_has_a_grammar() {
        for l in ["rust", "json", "toml", "bash", "ts", "tsx", "js", "c", "go", "python", "html", "css", "sql", "yaml"] {
            assert!(super::syntax(l).is_some(), "no grammar for {l}");
        }
        let h = super::html("rust", "fn main() { let x = \"s\"; } // c\n");
        assert!(h.contains("hl-keyword") || h.contains("hl-storage"), "{h}");
        assert!(h.contains("hl-string") && h.contains("hl-comment"), "{h}");
        assert_eq!(super::html("nope", "<a>"), "&lt;a&gt;");
    }
}
