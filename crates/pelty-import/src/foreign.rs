//! Shared helpers for converting other billing systems (Blesta, Paymenter)
//! into the WHMCS-shaped [`WhmcsData`](crate::whmcs::WhmcsData) model, so a
//! single, tested writer (`import::run_source`) does all the inserts.
use crate::whmcs::{Currency, Pricing};
use rust_decimal::Decimal;
use std::collections::HashMap;

/// Assigns stable numeric ids to ISO currency codes.
#[derive(Default)]
pub struct Currencies {
    ids: HashMap<String, i64>,
    pub rows: Vec<Currency>,
}

impl Currencies {
    pub fn id(&mut self, code: &str) -> i64 {
        let code = code.trim().to_ascii_uppercase();
        let code = if code.is_empty() {
            "EUR".to_string()
        } else {
            code
        };
        if let Some(id) = self.ids.get(&code) {
            return *id;
        }
        let id = self.rows.len() as i64 + 1;
        self.rows.push(Currency {
            id,
            code: code.clone(),
            default: id == 1,
        });
        self.ids.insert(code, id);
        id
    }
}

/// Maps a `(unit, length)` billing period to a WHMCS cycle name.
/// Units: `day|week|month|year|onetime|free`.
pub fn cycle(unit: &str, length: i64) -> Option<&'static str> {
    Some(
        match (
            unit.trim().to_ascii_lowercase().trim_end_matches('s'),
            length,
        ) {
            ("month", 1) => "Monthly",
            ("month", 3) => "Quarterly",
            ("month", 6) => "Semi-Annually",
            ("month", 12) | ("year", 1) => "Annually",
            ("month", 24) | ("year", 2) => "Biennially",
            ("month", 36) | ("year", 3) => "Triennially",
            ("onetime", _) | ("one-time", _) => "One Time",
            ("free", _) => "Free Account",
            _ => return None,
        },
    )
}

/// Sets one cycle's price + setup fee on a WHMCS pricing row. One-time prices
/// go into `monthly`, exactly like WHMCS does for one-time products.
pub fn set_price(p: &mut Pricing, cycle: &str, price: Decimal, setup: Decimal) {
    let (c, s) = match cycle {
        "Monthly" | "One Time" | "Free Account" => (&mut p.monthly, &mut p.msetupfee),
        "Quarterly" => (&mut p.quarterly, &mut p.qsetupfee),
        "Semi-Annually" => (&mut p.semiannually, &mut p.ssetupfee),
        "Annually" => (&mut p.annually, &mut p.asetupfee),
        "Biennially" => (&mut p.biennially, &mut p.bsetupfee),
        _ => (&mut p.triennially, &mut p.tsetupfee),
    };
    *c = price;
    *s = setup;
}

/// A pricing row where every cycle is disabled (`-1.00`, the WHMCS convention).
pub fn disabled_pricing(relid: i64, currency: i64) -> Pricing {
    let off = Decimal::NEGATIVE_ONE;
    Pricing {
        relid,
        currency,
        monthly: off,
        quarterly: off,
        semiannually: off,
        annually: off,
        biennially: off,
        triennially: off,
        ..Default::default()
    }
}

/// Accepts numbers or numeric strings from JSON exports, exact (no floats).
pub fn dec<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Decimal, D::Error> {
    use serde::Deserialize;
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::Null => Ok(Decimal::ZERO),
        serde_json::Value::String(s) if s.trim().is_empty() => Ok(Decimal::ZERO),
        serde_json::Value::String(s) => s.trim().parse().map_err(serde::de::Error::custom),
        serde_json::Value::Number(n) => n.to_string().parse().map_err(serde::de::Error::custom),
        other => Err(serde::de::Error::custom(format!("not a decimal: {other}"))),
    }
}

/// Accepts strings, numbers, booleans or null as a string.
pub fn text<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    use serde::Deserialize;
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    })
}

/// Accepts ints or numeric strings (MySQL JSON exports often quote ids).
pub fn int<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    use serde::Deserialize;
    match serde_json::Value::deserialize(d)? {
        serde_json::Value::Null => Ok(0),
        serde_json::Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| serde::de::Error::custom("bad int")),
        serde_json::Value::String(s) if s.trim().is_empty() => Ok(0),
        serde_json::Value::String(s) => s.trim().parse().map_err(serde::de::Error::custom),
        serde_json::Value::Bool(b) => Ok(i64::from(b)),
        other => Err(serde::de::Error::custom(format!("not an int: {other}"))),
    }
}

/// Accepts `true/false`, `0/1`, `"0"/"1"`, `"yes"/"no"`.
pub fn flag<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    use serde::Deserialize;
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Bool(b) => b,
        serde_json::Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
        serde_json::Value::String(s) => matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles() {
        assert_eq!(cycle("month", 1), Some("Monthly"));
        assert_eq!(cycle("months", 6), Some("Semi-Annually"));
        assert_eq!(cycle("year", 1), Some("Annually"));
        assert_eq!(cycle("year", 3), Some("Triennially"));
        assert_eq!(cycle("week", 1), None);
        assert_eq!(cycle("onetime", 0), Some("One Time"));
    }

    #[test]
    fn currency_ids_are_stable() {
        let mut c = Currencies::default();
        assert_eq!(c.id("eur"), 1);
        assert_eq!(c.id("USD"), 2);
        assert_eq!(c.id("EUR"), 1);
        assert!(c.rows[0].default && !c.rows[1].default);
    }

    #[test]
    fn decimals_from_json_are_exact() {
        #[derive(serde::Deserialize)]
        struct T {
            #[serde(deserialize_with = "dec")]
            a: Decimal,
        }
        let t: T = serde_json::from_str(r#"{"a":"10.10"}"#).unwrap();
        assert_eq!(t.a.to_string(), "10.10");
        let t: T = serde_json::from_str(r#"{"a":0.1}"#).unwrap();
        assert_eq!(t.a.to_string(), "0.1");
    }
}
