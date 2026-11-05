//! Tax engine: rules per country/region, inclusive or exclusive prices,
//! compound taxes and EU VAT reverse charge. Exact decimal; every tax amount
//! is rounded to the currency exponent per line, so lines always add up.
use crate::math::{round, MathError};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    /// Percent, e.g. 19 for 19 %.
    pub rate: Decimal,
    /// Compound taxes apply on top of net + all earlier (lower priority) taxes.
    pub compound: bool,
    /// VAT-type rules are skipped under EU reverse charge.
    pub is_vat: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaxPart {
    pub name: String,
    pub rate: Decimal,
    pub compound: bool,
    pub amount: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaxedLine {
    /// Net amount (excl. tax).
    pub net: Decimal,
    pub tax: Decimal,
    pub parts: Vec<TaxPart>,
}

/// `factor` such that gross = net * factor (exact, before rounding).
fn factor(rules: &[Rule]) -> Result<Decimal, MathError> {
    let mut f = Decimal::ONE;
    let simple: Decimal = rules.iter().filter(|r| !r.compound).map(|r| r.rate).sum();
    f = f
        .checked_add(simple / Decimal::ONE_HUNDRED)
        .ok_or(MathError::Overflow)?;
    for r in rules.iter().filter(|r| r.compound) {
        f = f
            .checked_mul(Decimal::ONE + r.rate / Decimal::ONE_HUNDRED)
            .ok_or(MathError::Overflow)?;
    }
    Ok(f)
}

/// Effective combined rate in percent (4 dp), stored on invoice lines.
pub fn effective_rate(rules: &[Rule]) -> Result<Decimal, MathError> {
    Ok(((factor(rules)? - Decimal::ONE) * Decimal::ONE_HUNDRED).round_dp(4))
}

fn validate(rules: &[Rule]) -> Result<(), MathError> {
    if rules
        .iter()
        .any(|r| r.rate < Decimal::ZERO || r.rate > Decimal::ONE_HUNDRED)
    {
        return Err(MathError::InvalidTaxRate);
    }
    Ok(())
}

/// Taxes on a net amount (already rounded to the exponent). Simple taxes
/// first, then compound taxes in order.
pub fn on_net(net: Decimal, rules: &[Rule], exponent: u32) -> Result<TaxedLine, MathError> {
    validate(rules)?;
    let mut parts = Vec::with_capacity(rules.len());
    let mut acc = net;
    for r in rules.iter().filter(|r| !r.compound) {
        let a = round(
            net.checked_mul(r.rate).ok_or(MathError::Overflow)? / Decimal::ONE_HUNDRED,
            exponent,
        );
        parts.push(TaxPart {
            name: r.name.clone(),
            rate: r.rate,
            compound: false,
            amount: a,
        });
        acc += a;
    }
    for r in rules.iter().filter(|r| r.compound) {
        let a = round(
            acc.checked_mul(r.rate).ok_or(MathError::Overflow)? / Decimal::ONE_HUNDRED,
            exponent,
        );
        parts.push(TaxPart {
            name: r.name.clone(),
            rate: r.rate,
            compound: true,
            amount: a,
        });
        acc += a;
    }
    let tax = parts.iter().map(|p| p.amount).sum();
    Ok(TaxedLine { net, tax, parts })
}

/// Taxes contained in a gross (tax-inclusive) amount: net + tax == gross
/// exactly; any rounding remainder goes to the last tax part.
pub fn on_gross(gross: Decimal, rules: &[Rule], exponent: u32) -> Result<TaxedLine, MathError> {
    validate(rules)?;
    if rules.is_empty() {
        return Ok(TaxedLine {
            net: gross,
            tax: Decimal::ZERO,
            parts: vec![],
        });
    }
    let net = round(gross / factor(rules)?, exponent);
    let mut l = on_net(net, rules, exponent)?;
    let diff = gross - net - l.tax;
    if let Some(last) = l.parts.last_mut() {
        last.amount += diff;
    }
    l.tax = gross - net;
    Ok(l)
}

/// Sum parts by (name, rate, compound) for the invoice tax breakdown.
pub fn merge(into: &mut Vec<TaxPart>, parts: &[TaxPart]) {
    for p in parts {
        match into
            .iter_mut()
            .find(|q| q.name == p.name && q.rate == p.rate && q.compound == p.compound)
        {
            Some(q) => q.amount += p.amount,
            None => into.push(p.clone()),
        }
    }
}

pub const EU: &[&str] = &[
    "AT", "BE", "BG", "CY", "CZ", "DE", "DK", "EE", "ES", "FI", "FR", "GR", "HR", "HU", "IE", "IT",
    "LT", "LU", "LV", "MT", "NL", "PL", "PT", "RO", "SE", "SI", "SK",
];

/// VAT number prefix: Greece uses EL.
pub fn vat_prefix(country: &str) -> &str {
    if country == "GR" {
        "EL"
    } else {
        country
    }
}

/// Normalize: uppercase, strip spaces/dots/dashes, add the country prefix.
pub fn normalize_vat(country: &str, raw: &str) -> String {
    let s: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    let p = vat_prefix(country);
    if s.is_empty() || s.starts_with(p) {
        s
    } else {
        format!("{p}{s}")
    }
}

/// Format check of an EU VAT number (country prefix + per-country shape).
/// Not a validity check; see [`VatValidator`] for VIES.
pub fn vat_format_ok(vat: &str) -> bool {
    if vat.len() < 4 || !vat.is_ascii() {
        return false;
    }
    let (p, b) = vat.split_at(2);
    let digits = |n: std::ops::RangeInclusive<usize>| {
        n.contains(&b.len()) && b.bytes().all(|c| c.is_ascii_digit())
    };
    let alnum = |n: std::ops::RangeInclusive<usize>| {
        n.contains(&b.len()) && b.bytes().all(|c| c.is_ascii_alphanumeric())
    };
    match p {
        "AT" => b.len() == 9 && b.starts_with('U') && b[1..].bytes().all(|c| c.is_ascii_digit()),
        "BE" => digits(10..=10),
        "BG" => digits(9..=10),
        "CY" => {
            b.len() == 9
                && b[..8].bytes().all(|c| c.is_ascii_digit())
                && b.as_bytes()[8].is_ascii_uppercase()
        }
        "CZ" => digits(8..=10),
        "DE" | "EE" | "EL" | "PT" => digits(9..=9),
        "DK" | "FI" | "HU" | "LU" | "MT" | "SI" => digits(8..=8),
        "ES" | "IE" => alnum(8..=9),
        "FR" => alnum(11..=11),
        "HR" | "IT" | "LV" => digits(11..=11),
        "LT" => digits(9..=12),
        "NL" => b.len() == 12 && b.as_bytes()[9] == b'B' && alnum(12..=12),
        "PL" | "SK" => digits(10..=10),
        "RO" => digits(2..=10),
        "SE" => digits(12..=12),
        _ => false,
    }
}

/// Pluggable VAT number verification (e.g. a VIES client). The default
/// implementation only checks the format and never touches the network.
pub trait VatValidator: Send + Sync {
    fn validate(&self, vat: &str) -> Result<bool, String>;
}

pub struct FormatOnly;
impl VatValidator for FormatOnly {
    fn validate(&self, vat: &str) -> Result<bool, String> {
        Ok(vat_format_ok(vat))
    }
}

/// Reverse charge applies to B2B supplies to another EU member state when the
/// buyer has a (format-)valid VAT number.
pub fn reverse_charge(home: &str, customer_country: &str, vat: &str, v: &dyn VatValidator) -> bool {
    !home.is_empty()
        && customer_country != home
        && EU.contains(&home)
        && EU.contains(&customer_country)
        && !vat.is_empty()
        && vat.starts_with(vat_prefix(customer_country))
        && v.validate(vat).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }
    fn r(name: &str, rate: &str, compound: bool) -> Rule {
        Rule {
            name: name.into(),
            rate: d(rate),
            compound,
            is_vat: true,
        }
    }

    #[test]
    fn exclusive_simple_and_compound() {
        let l = on_net(d("100.00"), &[r("VAT", "19", false)], 2).unwrap();
        assert_eq!((l.net, l.tax), (d("100.00"), d("19.00")));
        // Quebec-style: GST 5 % then QST 9.975 % compound on net+GST.
        let l = on_net(
            d("100.00"),
            &[r("GST", "5", false), r("QST", "9.975", true)],
            2,
        )
        .unwrap();
        assert_eq!(l.parts[1].amount, d("10.47")); // 105 * 9.975 % = 10.47375
        assert_eq!(l.tax, d("15.47"));
        assert_eq!(
            effective_rate(&[r("GST", "5", false), r("QST", "9.975", true)]).unwrap(),
            d("15.4738")
        );
    }

    #[test]
    fn inclusive_adds_up_exactly() {
        let l = on_gross(d("119.00"), &[r("VAT", "19", false)], 2).unwrap();
        assert_eq!((l.net, l.tax), (d("100.00"), d("19.00")));
        for g in ["0.01", "9.99", "33.33", "1234.57"] {
            let l = on_gross(d(g), &[r("A", "5", false), r("B", "9.975", true)], 2).unwrap();
            assert_eq!(l.net + l.tax, d(g));
            assert_eq!(l.parts.iter().map(|p| p.amount).sum::<Decimal>(), l.tax);
        }
        let jpy = on_gross(d("1100"), &[r("JCT", "10", false)], 0).unwrap();
        assert_eq!((jpy.net, jpy.tax), (d("1000"), d("100")));
    }

    #[test]
    fn rejects_bad_rate() {
        assert_eq!(
            on_net(d("1"), &[r("X", "101", false)], 2),
            Err(MathError::InvalidTaxRate)
        );
    }

    #[test]
    fn vat_numbers_and_reverse_charge() {
        assert_eq!(normalize_vat("DE", "de 123.456-789"), "DE123456789");
        assert_eq!(normalize_vat("GR", "123456789"), "EL123456789");
        assert!(vat_format_ok("DE123456789"));
        assert!(vat_format_ok("ATU12345678"));
        assert!(vat_format_ok("NL123456789B01"));
        assert!(vat_format_ok("RO12345678"));
        assert!(!vat_format_ok("DE12345"));
        assert!(!vat_format_ok("XX123456789"));
        assert!(reverse_charge("RO", "DE", "DE123456789", &FormatOnly));
        assert!(
            !reverse_charge("RO", "RO", "RO12345678", &FormatOnly),
            "domestic"
        );
        assert!(!reverse_charge("RO", "US", "", &FormatOnly), "non-EU");
        assert!(!reverse_charge("RO", "DE", "", &FormatOnly), "B2C");
        struct Vies(bool);
        impl VatValidator for Vies {
            fn validate(&self, _: &str) -> Result<bool, String> {
                Ok(self.0)
            }
        }
        assert!(
            !reverse_charge("RO", "DE", "DE123456789", &Vies(false)),
            "VIES says invalid"
        );
    }

    #[test]
    fn merge_breakdown() {
        let mut b = vec![];
        merge(
            &mut b,
            &on_net(d("10"), &[r("VAT", "19", false)], 2).unwrap().parts,
        );
        merge(
            &mut b,
            &on_net(d("20"), &[r("VAT", "19", false)], 2).unwrap().parts,
        );
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].amount, d("5.70"));
    }
}
