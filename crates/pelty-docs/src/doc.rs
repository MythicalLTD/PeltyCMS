//! Tiny document model rendered to HTML and Markdown, plus a parser for the
//! Markdown subset used by `templates/` (headings, paragraphs, `-` lists,
//! fenced code, tables via directives).
use std::fmt::Write;

#[derive(Debug, Clone)]
pub enum Block {
    H(u8, String),
    P(String),
    List(Vec<String>),
    Code(String, String),
    Table(Vec<String>, Vec<Vec<String>>),
    /// Pre-rendered HTML with a Markdown fallback.
    Raw(String, String),
}

#[derive(Debug, Clone)]
pub struct Page {
    pub slug: String,
    pub title: String,
    pub section: &'static str,
    pub blocks: Vec<Block>,
}

impl Page {
    pub fn new(slug: &str, title: &str, section: &'static str) -> Self {
        Page {
            slug: slug.into(),
            title: title.into(),
            section,
            blocks: vec![],
        }
    }
    pub fn h(&mut self, l: u8, t: impl Into<String>) -> &mut Self {
        self.blocks.push(Block::H(l, t.into()));
        self
    }
    pub fn p(&mut self, t: impl Into<String>) -> &mut Self {
        self.blocks.push(Block::P(t.into()));
        self
    }
    pub fn code(&mut self, lang: &str, t: impl Into<String>) -> &mut Self {
        self.blocks.push(Block::Code(lang.into(), t.into()));
        self
    }
    pub fn list(&mut self, items: Vec<String>) -> &mut Self {
        self.blocks.push(Block::List(items));
        self
    }
    pub fn table(&mut self, head: &[&str], rows: Vec<Vec<String>>) -> &mut Self {
        self.blocks.push(Block::Table(
            head.iter().map(|s| s.to_string()).collect(),
            rows,
        ));
        self
    }
}

/// A fence spec is `lang` or `lang title=path/to/file`.
pub fn code_spec(spec: &str) -> (&str, Option<&str>) {
    match spec.split_once(" title=") {
        Some((l, t)) => (l.trim(), Some(t.trim())),
        None => (spec.trim(), None),
    }
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn slugify(s: &str) -> String {
    let mut o = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            o.push(c);
        } else if !o.ends_with('-') {
            o.push('-');
        }
    }
    o.trim_matches('-').to_string()
}

/// Inline markup: `code`, **bold**, [text](href). Links ending in `.md` are
/// rewritten to `.html`.
pub fn inline(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('`') {
            if let Some(e) = r.find('`') {
                let _ = write!(out, "<code>{}</code>", esc(&r[..e]));
                rest = &r[e + 1..];
                continue;
            }
        }
        if let Some(r) = rest.strip_prefix("**") {
            if let Some(e) = r.find("**") {
                let _ = write!(out, "<strong>{}</strong>", inline(&r[..e]));
                rest = &r[e + 2..];
                continue;
            }
        }
        if let Some(r) = rest.strip_prefix('[') {
            if let Some(e) = r.find("](") {
                if let Some(c) = r[e + 2..].find(')') {
                    let href = &r[e + 2..e + 2 + c];
                    let href = href
                        .strip_suffix(".md")
                        .map(|h| format!("{h}.html"))
                        .unwrap_or_else(|| href.to_string());
                    let _ = write!(out, "<a href=\"{}\">{}</a>", esc(&href), inline(&r[..e]));
                    rest = &r[e + 3 + c..];
                    continue;
                }
            }
        }
        let c = rest.chars().next().unwrap();
        out.push_str(&esc(&c.to_string()));
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Inline markup without links (for the table of contents).
pub fn inline_text(s: &str) -> String {
    inline(
        &regex::Regex::new(r"\[([^\]]*)\]\([^)]*\)")
            .unwrap()
            .replace_all(s, "$1"),
    )
}

pub fn to_html(p: &Page) -> (String, Vec<(u8, String, String)>) {
    let mut o = String::new();
    let mut toc = vec![];
    for b in &p.blocks {
        match b {
            Block::H(l, t) => {
                let id = slugify(t);
                if *l <= 3 {
                    toc.push((*l, t.clone(), id.clone()));
                }
                let _ = writeln!(
                    o,
                    "<h{l} id=\"{id}\"><a class=\"anchor\" href=\"#{id}\">#</a>{}</h{l}>",
                    inline(t)
                );
            }
            Block::P(t) => {
                let _ = writeln!(o, "<p>{}</p>", inline(t));
            }
            Block::List(items) => {
                o.push_str("<ul>");
                for i in items {
                    let _ = write!(o, "<li>{}</li>", inline(i));
                }
                o.push_str("</ul>\n");
            }
            Block::Code(spec, t) => {
                let (lang, title) = code_spec(spec);
                let label = crate::highlight::label(lang);
                let head = match title {
                    Some(t) => format!("<span class=\"code-title\" title=\"{0}\">{0}</span><span class=\"code-lang\">{1}</span>", esc(t), esc(label)),
                    None => format!("<span class=\"code-lang solo\">{}</span>", esc(label)),
                };
                let _ = writeln!(
                    o,
                    "<figure class=\"code\"><div class=\"code-bar\">{head}<span class=\"code-actions\"><button type=\"button\" class=\"code-btn code-wrap\" aria-pressed=\"false\" title=\"Wrap long lines\">Wrap</button><button type=\"button\" class=\"code-btn code-copy\" aria-label=\"Copy code to clipboard\">Copy</button></span></div><pre class=\"code-body\"><code class=\"lang-{}\">{}</code></pre></figure>",
                    esc(lang),
                    crate::highlight::html(lang, t.trim_end_matches('\n'))
                );
            }
            Block::Table(h, rows) => {
                o.push_str("<div class=\"tw\"><table><thead><tr>");
                for c in h {
                    let _ = write!(o, "<th>{}</th>", inline(c));
                }
                o.push_str("</tr></thead><tbody>");
                for r in rows {
                    o.push_str("<tr>");
                    for c in r {
                        let _ = write!(o, "<td>{}</td>", inline(c));
                    }
                    o.push_str("</tr>");
                }
                o.push_str("</tbody></table></div>\n");
            }
            Block::Raw(h, _) => o.push_str(h),
        }
    }
    (o, toc)
}

pub fn to_md(p: &Page) -> String {
    let mut o = format!("# {}\n\n", p.title);
    let cell = |s: &str| s.replace('|', "\\|").replace('\n', " ");
    for b in &p.blocks {
        match b {
            Block::H(l, t) => {
                let _ = write!(o, "{} {t}\n\n", "#".repeat(*l as usize));
            }
            Block::P(t) => {
                let _ = write!(o, "{t}\n\n");
            }
            Block::List(items) => {
                for i in items {
                    let _ = writeln!(o, "- {i}");
                }
                o.push('\n');
            }
            Block::Code(spec, t) => {
                let (lang, title) = code_spec(spec);
                if let Some(t) = title {
                    let _ = write!(o, "_{t}_\n\n");
                }
                let _ = write!(o, "```{lang}\n{}\n```\n\n", t.trim_end());
            }
            Block::Table(h, rows) => {
                let _ = writeln!(
                    o,
                    "| {} |",
                    h.iter().map(|c| cell(c)).collect::<Vec<_>>().join(" | ")
                );
                let _ = writeln!(o, "|{}", "---|".repeat(h.len()));
                for r in rows {
                    let _ = writeln!(
                        o,
                        "| {} |",
                        r.iter().map(|c| cell(c)).collect::<Vec<_>>().join(" | ")
                    );
                }
                o.push('\n');
            }
            Block::Raw(_, md) => {
                o.push_str(md);
                o.push('\n');
            }
        }
    }
    o
}

/// Parse a template. `expand(directive)` resolves `{{name:arg}}` lines into blocks.
pub fn parse_template(
    src: &str,
    expand: &mut dyn FnMut(&str) -> Vec<Block>,
) -> (String, Vec<Block>) {
    let mut title = String::new();
    let mut blocks = vec![];
    let mut para: Vec<String> = vec![];
    let mut list: Vec<String> = vec![];
    let mut lines = src.lines();
    fn flush(para: &mut Vec<String>, list: &mut Vec<String>, blocks: &mut Vec<Block>) {
        if !para.is_empty() {
            blocks.push(Block::P(para.join(" ")));
            para.clear();
        }
        if !list.is_empty() {
            blocks.push(Block::List(std::mem::take(list)));
        }
    }
    while let Some(l) = lines.next() {
        let t = l.trim_end();
        if let Some(f) = t.strip_prefix("```") {
            flush(&mut para, &mut list, &mut blocks);
            let mut code = vec![];
            for c in lines.by_ref() {
                if c.trim_end() == "```" {
                    break;
                }
                code.push(c);
            }
            blocks.push(Block::Code(f.to_string(), code.join("\n")));
        } else if t.starts_with("{{") && t.ends_with("}}") {
            flush(&mut para, &mut list, &mut blocks);
            blocks.extend(expand(&t[2..t.len() - 2]));
        } else if let Some(h) = t.strip_prefix("# ") {
            title = h.to_string();
        } else if t.starts_with("##") {
            flush(&mut para, &mut list, &mut blocks);
            let n = t.chars().take_while(|c| *c == '#').count();
            blocks.push(Block::H(n as u8, t[n..].trim().to_string()));
        } else if let Some(i) = t.strip_prefix("- ") {
            if !para.is_empty() {
                flush(&mut para, &mut vec![], &mut blocks);
            }
            list.push(i.to_string());
        } else if t.starts_with("  ") && !list.is_empty() {
            let last = list.last_mut().unwrap();
            last.push(' ');
            last.push_str(t.trim());
        } else if t.is_empty() {
            flush(&mut para, &mut list, &mut blocks);
        } else {
            if !list.is_empty() {
                flush(&mut vec![], &mut list, &mut blocks);
            }
            para.push(t.to_string());
        }
    }
    flush(&mut para, &mut list, &mut blocks);
    (title, blocks)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inline_markup() {
        assert_eq!(
            inline("a `<b>` [x](events.md) **y**"),
            "a <code>&lt;b&gt;</code> <a href=\"events.html\">x</a> <strong>y</strong>"
        );
    }
    #[test]
    fn template_blocks() {
        let (t, b) = parse_template(
            "# T\n\nhello\nworld\n\n- a\n- b\n\n```rust\nfn x() {}\n```\n{{x:y}}\n",
            &mut |d| vec![Block::P(d.into())],
        );
        assert_eq!(t, "T");
        assert!(matches!(&b[0], Block::P(p) if p == "hello world"));
        assert!(matches!(&b[1], Block::List(l) if l.len() == 2));
        assert!(matches!(&b[2], Block::Code(l, c) if l == "rust" && c == "fn x() {}"));
        assert!(matches!(&b[3], Block::P(p) if p == "x:y"));
    }
}
