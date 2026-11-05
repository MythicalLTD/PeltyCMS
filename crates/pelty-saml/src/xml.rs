//! Minimal namespace-aware XML tree built on quick-xml.
//!
//! The tree keeps the original prefixes and namespace declarations, which
//! canonicalization needs. DOCTYPE is rejected (no entity expansion, no XXE).
//! Line endings and attribute values are normalized as the XML spec requires,
//! so the canonical form matches what the signer saw.
use crate::Error;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::BTreeMap;

pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const MAX_DEPTH: usize = 64;
const MAX_ELEMENTS: usize = 20_000;
/// Inputs larger than this are refused before parsing.
pub const MAX_INPUT: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Attr {
    pub prefix: String,
    pub local: String,
    pub ns: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub enum Child {
    Elem(usize),
    Text(String),
    Pi(String, String),
}

#[derive(Debug, Clone)]
pub struct Elem {
    pub prefix: String,
    pub local: String,
    pub ns: String,
    pub attrs: Vec<Attr>,
    /// In-scope namespace bindings (prefix -> uri; "" is the default namespace).
    pub scope: BTreeMap<String, String>,
    pub children: Vec<Child>,
    pub parent: Option<usize>,
}

#[derive(Debug)]
pub struct Doc {
    pub elems: Vec<Elem>,
    pub root: usize,
}

// quick-xml 0.42 exposes text/attrs as UTF-8 `&str` / `Cow<str>`.

fn split(q: &str) -> (&str, &str) {
    match q.split_once(':') {
        Some((p, l)) => (p, l),
        None => ("", q),
    }
}

fn unescape(s: &str) -> Result<String, Error> {
    quick_xml::escape::unescape(s)
        .map(|c| c.into_owned())
        .map_err(|_| Error::Xml("entity"))
}

impl Doc {
    pub fn parse(input: &str) -> Result<Doc, Error> {
        if input.len() > MAX_INPUT {
            return Err(Error::Xml("too_large"));
        }
        let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
        let mut r = Reader::from_str(&normalized);
        r.config_mut().check_end_names = true;
        r.config_mut().expand_empty_elements = false;
        let mut elems: Vec<Elem> = vec![];
        let mut stack: Vec<usize> = vec![];
        let mut root = None;
        loop {
            let ev = r.read_event().map_err(|_| Error::Xml("syntax"))?;
            match ev {
                Event::Start(e) => {
                    let idx = Self::open(&mut elems, &stack, &e)?;
                    if stack.is_empty() {
                        if root.is_some() {
                            return Err(Error::Xml("multiple_roots"));
                        }
                        root = Some(idx);
                    }
                    stack.push(idx);
                    if stack.len() > MAX_DEPTH {
                        return Err(Error::Xml("too_deep"));
                    }
                }
                Event::Empty(e) => {
                    let idx = Self::open(&mut elems, &stack, &e)?;
                    if stack.is_empty() {
                        if root.is_some() {
                            return Err(Error::Xml("multiple_roots"));
                        }
                        root = Some(idx);
                    }
                }
                Event::End(_) => {
                    stack.pop().ok_or(Error::Xml("syntax"))?;
                }
                Event::Text(t) => {
                    let raw = t.as_ref();
                    match stack.last() {
                        Some(&p) => {
                            let v = unescape(raw)?;
                            push_text(&mut elems[p].children, v);
                        }
                        None if raw.trim().is_empty() => {}
                        None => return Err(Error::Xml("text_outside_root")),
                    }
                }
                // quick-xml >= 0.38 reports `&amp;` etc. as separate events.
                Event::GeneralRef(g) => {
                    let p = *stack.last().ok_or(Error::Xml("text_outside_root"))?;
                    let v = unescape(&format!("&{};", g.as_ref()))?;
                    push_text(&mut elems[p].children, v);
                }
                Event::CData(t) => {
                    let p = *stack.last().ok_or(Error::Xml("text_outside_root"))?;
                    elems[p].children.push(Child::Text(t.as_ref().to_string()));
                }
                Event::PI(pi) => {
                    if let Some(&p) = stack.last() {
                        let raw = pi.as_ref();
                        let (t, d) = raw
                            .split_once(|c: char| c.is_whitespace())
                            .unwrap_or((raw, ""));
                        elems[p]
                            .children
                            .push(Child::Pi(t.to_string(), d.trim_start().to_string()));
                    }
                }
                Event::Comment(_) | Event::Decl(_) => {}
                Event::DocType(_) => return Err(Error::Xml("doctype")),
                Event::Eof => break,
            }
            if elems.len() > MAX_ELEMENTS {
                return Err(Error::Xml("too_large"));
            }
        }
        if !stack.is_empty() {
            return Err(Error::Xml("unclosed"));
        }
        Ok(Doc {
            elems,
            root: root.ok_or(Error::Xml("empty"))?,
        })
    }

    fn open(elems: &mut Vec<Elem>, stack: &[usize], e: &BytesStart) -> Result<usize, Error> {
        let parent = stack.last().copied();
        let mut scope = match parent {
            Some(p) => elems[p].scope.clone(),
            None => BTreeMap::new(),
        };
        let mut raw_attrs = vec![];
        for a in e.attributes().with_checks(true) {
            let a = a.map_err(|_| Error::Xml("attribute"))?;
            let key = a.key.as_ref().to_string();
            // Attribute-value normalization: literal whitespace becomes a space.
            let raw = a.value.as_ref().replace(['\t', '\n'], " ");
            let value = unescape(&raw)?;
            if key == "xmlns" {
                scope.insert(String::new(), value);
            } else if let Some(p) = key.strip_prefix("xmlns:") {
                if value.is_empty() || p == "xmlns" || (p == "xml" && value != XML_NS) {
                    return Err(Error::Xml("namespace"));
                }
                scope.insert(p.to_string(), value);
            } else {
                raw_attrs.push((key, value));
            }
        }
        let resolve = |p: &str| -> Result<String, Error> {
            if p == "xml" {
                return Ok(XML_NS.into());
            }
            scope.get(p).cloned().ok_or(Error::Xml("unbound_prefix"))
        };
        let name = e.name().as_ref().to_string();
        let (prefix, local) = split(&name);
        let ns = if prefix.is_empty() {
            scope.get("").cloned().unwrap_or_default()
        } else {
            resolve(prefix)?
        };
        let mut attrs = vec![];
        for (k, v) in raw_attrs {
            let (ap, al) = split(&k);
            let ans = if ap.is_empty() {
                String::new()
            } else {
                resolve(ap)?
            };
            if attrs.iter().any(|x: &Attr| x.local == al && x.ns == ans) {
                return Err(Error::Xml("duplicate_attribute"));
            }
            attrs.push(Attr {
                prefix: ap.into(),
                local: al.into(),
                ns: ans,
                value: v,
            });
        }
        let idx = elems.len();
        elems.push(Elem {
            prefix: prefix.into(),
            local: local.into(),
            ns,
            attrs,
            scope,
            children: vec![],
            parent,
        });
        if let Some(p) = parent {
            elems[p].children.push(Child::Elem(idx));
        }
        Ok(idx)
    }

    pub fn e(&self, i: usize) -> &Elem {
        &self.elems[i]
    }

    pub fn is(&self, i: usize, ns: &str, local: &str) -> bool {
        let e = &self.elems[i];
        e.ns == ns && e.local == local
    }

    /// Un-namespaced attribute value.
    pub fn attr(&self, i: usize, local: &str) -> Option<&str> {
        self.elems[i]
            .attrs
            .iter()
            .find(|a| a.ns.is_empty() && a.local == local)
            .map(|a| a.value.as_str())
    }

    pub fn kids(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        self.elems[i].children.iter().filter_map(|c| {
            if let Child::Elem(x) = c {
                Some(*x)
            } else {
                None
            }
        })
    }

    pub fn kids_named<'a>(
        &'a self,
        i: usize,
        ns: &'a str,
        local: &'a str,
    ) -> impl Iterator<Item = usize> + 'a {
        self.kids(i).filter(move |&k| self.is(k, ns, local))
    }

    /// Exactly one child with this name, else `None`.
    pub fn only(&self, i: usize, ns: &str, local: &str) -> Option<usize> {
        let mut it = self.kids_named(i, ns, local);
        let first = it.next()?;
        if it.next().is_some() {
            return None;
        }
        Some(first)
    }

    /// Concatenated descendant text.
    pub fn text(&self, i: usize) -> String {
        let mut s = String::new();
        self.text_into(i, &mut s);
        s
    }
    fn text_into(&self, i: usize, out: &mut String) {
        for c in &self.elems[i].children {
            match c {
                Child::Text(t) => out.push_str(t),
                Child::Elem(x) => self.text_into(*x, out),
                Child::Pi(..) => {}
            }
        }
    }

    /// Every element index in document order.
    pub fn all(&self) -> impl Iterator<Item = usize> {
        0..self.elems.len()
    }

    pub fn is_descendant(&self, mut i: usize, of: usize) -> bool {
        while let Some(p) = self.elems[i].parent {
            if p == of {
                return true;
            }
            i = p;
        }
        false
    }
}

/// Appends text, merging with a preceding text node so entity references do
/// not split one logical text value into several children.
fn push_text(children: &mut Vec<Child>, v: String) {
    if let Some(Child::Text(prev)) = children.last_mut() {
        prev.push_str(&v);
    } else {
        children.push(Child::Text(v));
    }
}

#[cfg(test)]
mod entity_tests {
    use super::*;

    #[test]
    fn entity_refs_merge_into_one_text_node() {
        let d = Doc::parse("<a>x &amp; y &lt;z&gt; &#65;</a>").unwrap();
        let root = d.all().next().unwrap();
        assert_eq!(d.text(root), "x & y <z> A");
        assert_eq!(d.e(root).children.len(), 1);
        assert!(Doc::parse("<a>&bogus;</a>").is_err());
    }
}
