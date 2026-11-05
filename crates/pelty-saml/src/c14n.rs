//! Exclusive XML Canonicalization 1.0 (without comments),
//! <https://www.w3.org/TR/xml-exc-c14n/>, applied to an element subtree.
use crate::xml::{Child, Doc, XML_NS};
use std::collections::BTreeMap;

pub const EXC_C14N: &str = "http://www.w3.org/2001/10/xml-exc-c14n#";

fn esc_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#xD;"),
            _ => out.push(c),
        }
    }
}

fn esc_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#x9;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            _ => out.push(c),
        }
    }
}

/// Canonicalize the subtree rooted at `apex`. `exclude` (the enveloped
/// signature) is skipped with its descendants. `inclusive` is the
/// InclusiveNamespaces PrefixList (`#default` for the default namespace).
pub fn canonicalize(
    doc: &Doc,
    apex: usize,
    exclude: Option<usize>,
    inclusive: &[String],
) -> String {
    let inc: Vec<String> = inclusive
        .iter()
        .map(|p| {
            if p == "#default" {
                String::new()
            } else {
                p.clone()
            }
        })
        .collect();
    let mut out = String::new();
    elem(doc, apex, exclude, &inc, &BTreeMap::new(), &mut out);
    out
}

fn elem(
    doc: &Doc,
    i: usize,
    exclude: Option<usize>,
    inc: &[String],
    rendered: &BTreeMap<String, String>,
    out: &mut String,
) {
    let e = doc.e(i);
    let mut used: Vec<String> = vec![e.prefix.clone()];
    for a in &e.attrs {
        if !a.prefix.is_empty() && a.prefix != "xml" {
            used.push(a.prefix.clone());
        }
    }
    for p in inc {
        if e.scope.contains_key(p) {
            used.push(p.clone());
        }
    }
    let mut decls: BTreeMap<String, String> = BTreeMap::new();
    for p in used {
        if p == "xml" {
            continue;
        }
        let uri = e.scope.get(&p).cloned().unwrap_or_default();
        let prev = rendered.get(&p).cloned().unwrap_or_default();
        if p.is_empty() {
            if uri == prev {
                continue;
            }
        } else if rendered.get(&p) == Some(&uri) {
            continue;
        }
        decls.insert(p, uri);
    }
    let qname = if e.prefix.is_empty() {
        e.local.clone()
    } else {
        format!("{}:{}", e.prefix, e.local)
    };
    out.push('<');
    out.push_str(&qname);
    for (p, uri) in &decls {
        if p.is_empty() {
            out.push_str(" xmlns=\"");
        } else {
            out.push_str(" xmlns:");
            out.push_str(p);
            out.push_str("=\"");
        }
        esc_attr(uri, out);
        out.push('"');
    }
    let mut attrs: Vec<_> = e.attrs.iter().collect();
    attrs.sort_by(|a, b| (a.ns.as_str(), a.local.as_str()).cmp(&(b.ns.as_str(), b.local.as_str())));
    for a in attrs {
        out.push(' ');
        if !a.prefix.is_empty() {
            out.push_str(if a.ns == XML_NS { "xml" } else { &a.prefix });
            out.push(':');
        }
        out.push_str(&a.local);
        out.push_str("=\"");
        esc_attr(&a.value, out);
        out.push('"');
    }
    out.push('>');
    let mut next = rendered.clone();
    next.extend(decls);
    for c in &e.children {
        match c {
            Child::Elem(x) if Some(*x) == exclude => {}
            Child::Elem(x) => elem(doc, *x, exclude, inc, &next, out),
            Child::Text(t) => esc_text(t, out),
            Child::Pi(t, d) => {
                out.push_str("<?");
                out.push_str(t);
                if !d.is_empty() {
                    out.push(' ');
                    out.push_str(d);
                }
                out.push_str("?>");
            }
        }
    }
    out.push_str("</");
    out.push_str(&qname);
    out.push('>');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(xml: &str, path: &[&str]) -> String {
        let d = Doc::parse(xml).unwrap();
        let mut i = d.root;
        for p in path {
            i = d.kids(i).find(|&k| d.e(k).local == *p).unwrap();
        }
        canonicalize(&d, i, None, &[])
    }

    #[test]
    fn w3c_exc_c14n_example() {
        // From the Exclusive C14N spec, section 2.2.
        let xml = "<n0:local xmlns:n0=\"foo:bar\" xmlns:n3=\"ftp://example.org\"><n1:elem2 xmlns:n1=\"http://example.net\" xml:lang=\"en\"><n3:stuff xmlns:n3=\"ftp://example.org\"/></n1:elem2></n0:local>";
        assert_eq!(
            c(xml, &["elem2"]),
            "<n1:elem2 xmlns:n1=\"http://example.net\" xml:lang=\"en\"><n3:stuff xmlns:n3=\"ftp://example.org\"></n3:stuff></n1:elem2>"
        );
    }

    #[test]
    fn sorts_escapes_and_drops_comments() {
        let xml = "<?xml version=\"1.0\"?><r xmlns=\"urn:d\" xmlns:b=\"urn:b\" xmlns:a=\"urn:a\" z=\"1\" b:x=\"2\" a:y=\"3&#9;&quot;\"><!-- c --><s>&lt;t&gt; &amp; \r\nu</s><e/></r>";
        assert_eq!(
            c(xml, &[]),
            "<r xmlns=\"urn:d\" xmlns:a=\"urn:a\" xmlns:b=\"urn:b\" z=\"1\" a:y=\"3&#x9;&quot;\" b:x=\"2\"><s>&lt;t&gt; &amp; \nu</s><e></e></r>"
        );
    }

    #[test]
    fn default_namespace_undeclared() {
        let xml = "<r xmlns=\"urn:d\"><x xmlns=\"\"><y/></x></r>";
        assert_eq!(
            c(xml, &[]),
            "<r xmlns=\"urn:d\"><x xmlns=\"\"><y></y></x></r>"
        );
        assert_eq!(c(xml, &["x"]), "<x><y></y></x>");
    }

    #[test]
    fn inclusive_prefix_list() {
        let d = Doc::parse("<r xmlns:xs=\"urn:xs\" xmlns:p=\"urn:p\"><p:a v=\"xs:string\"/></r>")
            .unwrap();
        let a = d.kids(d.root).next().unwrap();
        assert_eq!(
            canonicalize(&d, a, None, &[]),
            "<p:a xmlns:p=\"urn:p\" v=\"xs:string\"></p:a>"
        );
        assert_eq!(
            canonicalize(&d, a, None, &["xs".into()]),
            "<p:a xmlns:p=\"urn:p\" xmlns:xs=\"urn:xs\" v=\"xs:string\"></p:a>"
        );
    }

    #[test]
    fn rejects_doctype() {
        assert!(Doc::parse("<!DOCTYPE r [<!ENTITY x \"y\">]><r>&x;</r>").is_err());
        assert!(Doc::parse("<r>&unknown;</r>").is_err());
    }
}
