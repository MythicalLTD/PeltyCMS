//! Minimal RFC 4180 CSV writer with spreadsheet formula-injection guard.
use rust_decimal::Decimal;

#[derive(Default)]
pub struct Csv(String);

impl Csv {
    pub fn new(header: &[&str]) -> Self {
        let mut c = Self(String::new());
        c.row(header.iter().map(|h| Cell::Text(h)));
        c
    }

    pub fn row<'a>(&mut self, cells: impl IntoIterator<Item = Cell<'a>>) {
        let mut first = true;
        for c in cells {
            if !first {
                self.0.push(',');
            }
            first = false;
            match c {
                Cell::Text(t) => self.text(t),
                Cell::Dec(d) => self.0.push_str(&dec(d)),
            }
        }
        self.0.push_str("\r\n");
    }

    fn text(&mut self, t: &str) {
        // A leading = + - @ (or tab/CR) would be evaluated as a formula.
        let guard = t.starts_with(['=', '+', '-', '@', '\t', '\r']);
        if guard || t.contains([',', '"', '\n', '\r']) {
            self.0.push('"');
            if guard {
                self.0.push('\'');
            }
            self.0.push_str(&t.replace('"', "\"\""));
            self.0.push('"');
        } else {
            self.0.push_str(t);
        }
    }

    pub fn finish(self) -> String {
        self.0
    }
}

pub enum Cell<'a> {
    Text(&'a str),
    Dec(Decimal),
}

/// Exact decimal with at least two places: 12.5 -> "12.50", 0.1234 kept.
pub fn dec(d: Decimal) -> String {
    let mut n = d.normalize();
    if n.scale() < 2 {
        n.rescale(2);
    }
    n.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn escapes_and_guards() {
        let mut c = Csv::new(&["a", "b"]);
        c.row([Cell::Text("x,y"), Cell::Text("say \"hi\"")]);
        c.row([Cell::Text("=SUM(A1)"), Cell::Dec(Decimal::from_str("-12.5000").unwrap())]);
        c.row([Cell::Text("Ștefan"), Cell::Dec(Decimal::from_str("0.1234").unwrap())]);
        assert_eq!(c.finish(), "a,b\r\n\"x,y\",\"say \"\"hi\"\"\"\r\n\"'=SUM(A1)\",-12.50\r\nȘtefan,0.1234\r\n");
        assert_eq!(dec(Decimal::from_str("100").unwrap()), "100.00");
        assert_eq!(dec(Decimal::from_str("0.000").unwrap()), "0.00");
    }
}
