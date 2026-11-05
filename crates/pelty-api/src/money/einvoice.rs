//! E-invoicing: UBL 2.1 export following Peppol BIS Billing 3.0 (EN 16931),
//! optionally with the Romanian CIUS-RO customization used by RO e-Factura.
//!
//! The builder is pure (no I/O) so it can be unit-tested; the route layer
//! collects the invoice, its lines, the customer snapshot and the configured
//! legal fields. Amounts are rounded to the currency exponent per line and
//! totals are derived from the rounded lines so the EN 16931 arithmetic
//! rules (BR-CO-10/13/15) always hold.
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write;

pub const PEPPOL_CUSTOMIZATION: &str =
    "urn:cen.eu:en16931:2017#compliant#urn:fdc:peppol.eu:2017:poacc:billing:3.0";
pub const CIUS_RO_CUSTOMIZATION: &str =
    "urn:cen.eu:en16931:2017#compliant#urn:efactura.mfinante.ro:CIUS-RO:1.0.1";
pub const PEPPOL_PROFILE: &str = "urn:fdc:peppol.eu:2017:poacc:billing:01:1.0";

/// Admin-configurable legal fields (settings key `einvoice`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// `peppol` (Peppol BIS 3.0) or `cius_ro` (RO e-Factura).
    pub profile: String,
    pub seller_name: String,
    /// VAT identifier with country prefix (e.g. `RO12345678`). Empty when not VAT registered.
    pub seller_vat_id: String,
    /// Legal registration id (e.g. RO trade register `J40/123/2020`).
    pub seller_company_id: String,
    /// Legal form / extra legal info (BT-33), e.g. share capital.
    pub seller_legal_info: String,
    pub seller_street: String,
    pub seller_city: String,
    /// ISO 3166-2 subdivision (CIUS-RO requires e.g. `RO-B`, `RO-CJ`).
    pub seller_subdivision: String,
    pub seller_postal_code: String,
    pub seller_country: String,
    pub seller_email: String,
    pub seller_phone: String,
    /// Peppol electronic address (BT-34) and its EAS scheme (e.g. `0088`, `9947`).
    pub endpoint_id: String,
    pub endpoint_scheme: String,
    pub iban: String,
    pub bic: String,
    /// UNCL4461 payment means code; default 30 (credit transfer).
    pub payment_means_code: String,
}

impl Settings {
    pub fn normalized(mut self) -> Result<Self, &'static str> {
        for v in [
            &mut self.profile,
            &mut self.seller_name,
            &mut self.seller_vat_id,
            &mut self.seller_company_id,
            &mut self.seller_legal_info,
            &mut self.seller_street,
            &mut self.seller_city,
            &mut self.seller_subdivision,
            &mut self.seller_postal_code,
            &mut self.seller_country,
            &mut self.seller_email,
            &mut self.seller_phone,
            &mut self.endpoint_id,
            &mut self.endpoint_scheme,
            &mut self.iban,
            &mut self.bic,
            &mut self.payment_means_code,
        ] {
            *v = v.trim().to_string();
            if v.len() > 300 {
                return Err("error.validation");
            }
        }
        if self.profile.is_empty() {
            self.profile = "peppol".into();
        }
        if !matches!(self.profile.as_str(), "peppol" | "cius_ro") {
            return Err("error.einvoice_profile");
        }
        self.seller_country = self.seller_country.to_ascii_uppercase();
        if !self.seller_country.is_empty()
            && !(self.seller_country.len() == 2
                && self.seller_country.bytes().all(|b| b.is_ascii_uppercase()))
        {
            return Err("error.invalid_country");
        }
        self.iban = self.iban.replace(' ', "").to_ascii_uppercase();
        if !self.endpoint_scheme.is_empty()
            && !(self.endpoint_scheme.len() == 4
                && self.endpoint_scheme.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err("error.einvoice_scheme");
        }
        if self.payment_means_code.is_empty() {
            self.payment_means_code = "30".into();
        }
        if !self
            .payment_means_code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric())
            || self.payment_means_code.len() > 3
        {
            return Err("error.validation");
        }
        Ok(self)
    }
}

/// Buyer data (from the customer snapshot frozen on the invoice).
#[derive(Debug, Clone, Default)]
pub struct Party {
    pub name: String,
    pub vat_id: String,
    pub company_id: String,
    pub street: String,
    pub city: String,
    pub subdivision: String,
    pub postal_code: String,
    pub country: String,
    pub email: String,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub description: String,
    pub quantity: Decimal,
    pub unit_price: Decimal,
    pub net: Decimal,
    pub rate: Decimal,
}

#[derive(Debug, Clone)]
pub struct Doc {
    /// `invoice`, `credit_note` or `debit_note`.
    pub kind: String,
    pub number: String,
    pub issue_date: String,
    pub due_date: Option<String>,
    pub currency: String,
    pub exponent: u32,
    pub reverse_charge: bool,
    pub prepaid: Decimal,
    /// Number of the invoice a credit/debit note refers to (BT-25).
    pub billing_reference: Option<String>,
    pub note: String,
    pub lines: Vec<Line>,
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' && c != '\r' => {}
            c => o.push(c),
        }
    }
    o
}

fn amt(v: Decimal, exp: u32) -> String {
    let r = v.round_dp_with_strategy(exp, RoundingStrategy::MidpointAwayFromZero);
    format!("{:.*}", exp as usize, r)
}

fn category(rate: Decimal, reverse: bool) -> (&'static str, Option<&'static str>) {
    if reverse {
        ("AE", Some("Reverse charge"))
    } else if rate > Decimal::ZERO {
        ("S", None)
    } else {
        ("E", Some("Exempt from VAT"))
    }
}

fn opt_tag(out: &mut String, tag: &str, v: &str) {
    if !v.is_empty() {
        let _ = write!(out, "<{tag}>{}</{tag}>", esc(v));
    }
}

fn party(
    out: &mut String,
    p: &Party,
    endpoint: Option<(&str, &str)>,
    legal_info: &str,
    phone: &str,
) {
    out.push_str("<cac:Party>");
    if let Some((id, scheme)) = endpoint {
        let _ = write!(
            out,
            "<cbc:EndpointID schemeID=\"{}\">{}</cbc:EndpointID>",
            esc(scheme),
            esc(id)
        );
    }
    out.push_str("<cac:PostalAddress>");
    opt_tag(out, "cbc:StreetName", &p.street);
    opt_tag(out, "cbc:CityName", &p.city);
    opt_tag(out, "cbc:PostalZone", &p.postal_code);
    opt_tag(out, "cbc:CountrySubentity", &p.subdivision);
    let _ = write!(
        out,
        "<cac:Country><cbc:IdentificationCode>{}</cbc:IdentificationCode></cac:Country>",
        esc(&p.country)
    );
    out.push_str("</cac:PostalAddress>");
    if !p.vat_id.is_empty() {
        let _ = write!(out, "<cac:PartyTaxScheme><cbc:CompanyID>{}</cbc:CompanyID><cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme></cac:PartyTaxScheme>", esc(&p.vat_id));
    }
    let _ = write!(
        out,
        "<cac:PartyLegalEntity><cbc:RegistrationName>{}</cbc:RegistrationName>",
        esc(&p.name)
    );
    opt_tag(out, "cbc:CompanyID", &p.company_id);
    opt_tag(out, "cbc:CompanyLegalForm", legal_info);
    out.push_str("</cac:PartyLegalEntity>");
    if !p.email.is_empty() || !phone.is_empty() {
        out.push_str("<cac:Contact>");
        opt_tag(out, "cbc:Telephone", phone);
        opt_tag(out, "cbc:ElectronicMail", &p.email);
        out.push_str("</cac:Contact>");
    }
    out.push_str("</cac:Party>");
}

/// Check the data required by EN 16931 before export; returns i18n keys.
pub fn missing(cfg: &Settings, buyer: &Party) -> Vec<&'static str> {
    let mut m = Vec::new();
    if cfg.seller_name.is_empty() {
        m.push("einvoice.missing.seller_name");
    }
    if cfg.seller_country.is_empty() {
        m.push("einvoice.missing.seller_country");
    }
    if buyer.name.is_empty() {
        m.push("einvoice.missing.buyer_name");
    }
    if buyer.country.is_empty() {
        m.push("einvoice.missing.buyer_country");
    }
    if cfg.profile == "cius_ro" {
        if cfg.seller_vat_id.is_empty() && cfg.seller_company_id.is_empty() {
            m.push("einvoice.missing.seller_vat_id");
        }
        if cfg.seller_street.is_empty() || cfg.seller_city.is_empty() {
            m.push("einvoice.missing.seller_address");
        }
        if cfg.seller_country == "RO" && cfg.seller_subdivision.is_empty() {
            m.push("einvoice.missing.seller_subdivision");
        }
    }
    m
}

/// Render the UBL document. Credit notes use the `CreditNote` root (type
/// 381); invoices use 380 and debit notes 383.
pub fn render(doc: &Doc, cfg: &Settings, buyer: &Party) -> String {
    let credit = doc.kind == "credit_note";
    let (root, ns, type_tag, type_code, line_tag, qty_tag) = if credit {
        (
            "CreditNote",
            "urn:oasis:names:specification:ubl:schema:xsd:CreditNote-2",
            "cbc:CreditNoteTypeCode",
            "381",
            "cac:CreditNoteLine",
            "cbc:CreditedQuantity",
        )
    } else {
        let code = if doc.kind == "debit_note" {
            "383"
        } else {
            "380"
        };
        (
            "Invoice",
            "urn:oasis:names:specification:ubl:schema:xsd:Invoice-2",
            "cbc:InvoiceTypeCode",
            code,
            "cac:InvoiceLine",
            "cbc:InvoicedQuantity",
        )
    };
    let exp = doc.exponent;
    let cur = esc(&doc.currency);
    let custom = if cfg.profile == "cius_ro" {
        CIUS_RO_CUSTOMIZATION
    } else {
        PEPPOL_CUSTOMIZATION
    };
    let mut o = String::with_capacity(4096);
    let _ = write!(
        o,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<{root} xmlns=\"{ns}\" xmlns:cac=\"urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2\" xmlns:cbc=\"urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2\">"
    );
    let _ = write!(o, "<cbc:CustomizationID>{custom}</cbc:CustomizationID><cbc:ProfileID>{PEPPOL_PROFILE}</cbc:ProfileID>");
    let _ = write!(
        o,
        "<cbc:ID>{}</cbc:ID><cbc:IssueDate>{}</cbc:IssueDate>",
        esc(&doc.number),
        esc(&doc.issue_date)
    );
    if !credit {
        if let Some(d) = &doc.due_date {
            let _ = write!(o, "<cbc:DueDate>{}</cbc:DueDate>", esc(d));
        }
    }
    let _ = write!(o, "<{type_tag}>{type_code}</{type_tag}>");
    opt_tag(&mut o, "cbc:Note", &doc.note);
    let _ = write!(
        o,
        "<cbc:DocumentCurrencyCode>{cur}</cbc:DocumentCurrencyCode>"
    );
    if let Some(r) = &doc.billing_reference {
        let _ = write!(o, "<cac:BillingReference><cac:InvoiceDocumentReference><cbc:ID>{}</cbc:ID></cac:InvoiceDocumentReference></cac:BillingReference>", esc(r));
    }
    // Seller
    let seller = Party {
        name: cfg.seller_name.clone(),
        vat_id: cfg.seller_vat_id.clone(),
        company_id: cfg.seller_company_id.clone(),
        street: cfg.seller_street.clone(),
        city: cfg.seller_city.clone(),
        subdivision: cfg.seller_subdivision.clone(),
        postal_code: cfg.seller_postal_code.clone(),
        country: cfg.seller_country.clone(),
        email: cfg.seller_email.clone(),
    };
    let ep = (!cfg.endpoint_id.is_empty() && !cfg.endpoint_scheme.is_empty())
        .then_some((cfg.endpoint_id.as_str(), cfg.endpoint_scheme.as_str()));
    o.push_str("<cac:AccountingSupplierParty>");
    party(
        &mut o,
        &seller,
        ep,
        &cfg.seller_legal_info,
        &cfg.seller_phone,
    );
    o.push_str("</cac:AccountingSupplierParty><cac:AccountingCustomerParty>");
    let bep = (!buyer.email.is_empty()).then_some((buyer.email.as_str(), "EM"));
    party(&mut o, buyer, bep, "", "");
    o.push_str("</cac:AccountingCustomerParty>");
    if !credit && !cfg.iban.is_empty() {
        let code = if cfg.payment_means_code.is_empty() {
            "30"
        } else {
            cfg.payment_means_code.as_str()
        };
        let _ = write!(o, "<cac:PaymentMeans><cbc:PaymentMeansCode>{}</cbc:PaymentMeansCode><cbc:PaymentID>{}</cbc:PaymentID><cac:PayeeFinancialAccount><cbc:ID>{}</cbc:ID>", esc(code), esc(&doc.number), esc(&cfg.iban));
        if !cfg.bic.is_empty() {
            let _ = write!(o, "<cac:FinancialInstitutionBranch><cbc:ID>{}</cbc:ID></cac:FinancialInstitutionBranch>", esc(&cfg.bic));
        }
        o.push_str("</cac:PayeeFinancialAccount></cac:PaymentMeans>");
    }
    // Tax subtotals grouped by (category, rate) over rounded line nets.
    let mut groups: BTreeMap<(&str, String), (Decimal, Decimal)> = BTreeMap::new();
    let mut line_total = Decimal::ZERO;
    for l in &doc.lines {
        let net = l
            .net
            .round_dp_with_strategy(exp, RoundingStrategy::MidpointAwayFromZero);
        line_total += net;
        let rate = if doc.reverse_charge {
            Decimal::ZERO
        } else {
            l.rate
        };
        let (cat, _) = category(rate, doc.reverse_charge);
        let e = groups
            .entry((cat, rate.normalize().to_string()))
            .or_insert((Decimal::ZERO, rate));
        e.0 += net;
    }
    let mut tax_total = Decimal::ZERO;
    let mut sub = String::new();
    for ((cat, rate_s), (base, rate)) in &groups {
        let tax = (base * rate / Decimal::ONE_HUNDRED)
            .round_dp_with_strategy(exp, RoundingStrategy::MidpointAwayFromZero);
        tax_total += tax;
        let _ = write!(
            sub,
            "<cac:TaxSubtotal><cbc:TaxableAmount currencyID=\"{cur}\">{}</cbc:TaxableAmount><cbc:TaxAmount currencyID=\"{cur}\">{}</cbc:TaxAmount><cac:TaxCategory><cbc:ID>{cat}</cbc:ID><cbc:Percent>{rate_s}</cbc:Percent>",
            amt(*base, exp),
            amt(tax, exp)
        );
        if let (_, Some(reason)) = category(*rate, doc.reverse_charge) {
            let _ = write!(
                sub,
                "<cbc:TaxExemptionReason>{reason}</cbc:TaxExemptionReason>"
            );
        }
        sub.push_str("<cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme></cac:TaxCategory></cac:TaxSubtotal>");
    }
    let _ = write!(
        o,
        "<cac:TaxTotal><cbc:TaxAmount currencyID=\"{cur}\">{}</cbc:TaxAmount>{sub}</cac:TaxTotal>",
        amt(tax_total, exp)
    );
    let incl = line_total + tax_total;
    let prepaid = doc.prepaid.min(incl).max(Decimal::ZERO);
    let _ = write!(
        o,
        "<cac:LegalMonetaryTotal><cbc:LineExtensionAmount currencyID=\"{cur}\">{le}</cbc:LineExtensionAmount><cbc:TaxExclusiveAmount currencyID=\"{cur}\">{le}</cbc:TaxExclusiveAmount><cbc:TaxInclusiveAmount currencyID=\"{cur}\">{inc}</cbc:TaxInclusiveAmount>",
        le = amt(line_total, exp),
        inc = amt(incl, exp)
    );
    if prepaid > Decimal::ZERO {
        let _ = write!(
            o,
            "<cbc:PrepaidAmount currencyID=\"{cur}\">{}</cbc:PrepaidAmount>",
            amt(prepaid, exp)
        );
    }
    let _ = write!(
        o,
        "<cbc:PayableAmount currencyID=\"{cur}\">{}</cbc:PayableAmount></cac:LegalMonetaryTotal>",
        amt(incl - prepaid, exp)
    );
    for (i, l) in doc.lines.iter().enumerate() {
        let rate = if doc.reverse_charge {
            Decimal::ZERO
        } else {
            l.rate
        };
        let (cat, _) = category(rate, doc.reverse_charge);
        let name: String = l
            .description
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(100)
            .collect();
        let _ = write!(
            o,
            "<{line_tag}><cbc:ID>{}</cbc:ID><{qty_tag} unitCode=\"C62\">{}</{qty_tag}><cbc:LineExtensionAmount currencyID=\"{cur}\">{}</cbc:LineExtensionAmount><cac:Item>",
            i + 1,
            l.quantity.normalize(),
            amt(l.net, exp)
        );
        if name != l.description {
            opt_tag(&mut o, "cbc:Description", &l.description);
        }
        let _ = write!(
            o,
            "<cbc:Name>{}</cbc:Name><cac:ClassifiedTaxCategory><cbc:ID>{cat}</cbc:ID><cbc:Percent>{}</cbc:Percent><cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme></cac:ClassifiedTaxCategory></cac:Item><cac:Price><cbc:PriceAmount currencyID=\"{cur}\">{}</cbc:PriceAmount></cac:Price></{line_tag}>",
            esc(&name),
            rate.normalize(),
            l.unit_price.normalize()
        );
    }
    let _ = write!(o, "</{root}>");
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn cfg() -> Settings {
        Settings {
            profile: "cius_ro".into(),
            seller_name: "Mythical & Co SRL".into(),
            seller_vat_id: "RO12345678".into(),
            seller_company_id: "J40/1/2020".into(),
            seller_street: "Str. X 1".into(),
            seller_city: "SECTOR1".into(),
            seller_subdivision: "RO-B".into(),
            seller_country: "RO".into(),
            iban: "RO49AAAA1B31007593840000".into(),
            ..Default::default()
        }
        .normalized()
        .unwrap()
    }
    fn buyer() -> Party {
        Party {
            name: "Acme <GmbH>".into(),
            country: "DE".into(),
            vat_id: "DE123".into(),
            email: "a@b.de".into(),
            ..Default::default()
        }
    }
    fn doc(kind: &str) -> Doc {
        Doc {
            kind: kind.into(),
            number: "INV-000001".into(),
            issue_date: "2026-10-30".into(),
            due_date: Some("2026-11-06".into()),
            currency: "EUR".into(),
            exponent: 2,
            reverse_charge: false,
            prepaid: dec("0"),
            billing_reference: None,
            note: String::new(),
            lines: vec![
                Line {
                    description: "VPS S".into(),
                    quantity: dec("1"),
                    unit_price: dec("10.005"),
                    net: dec("10.005"),
                    rate: dec("19"),
                },
                Line {
                    description: "Domain".into(),
                    quantity: dec("2"),
                    unit_price: dec("5"),
                    net: dec("10"),
                    rate: dec("19"),
                },
                Line {
                    description: "Book".into(),
                    quantity: dec("1"),
                    unit_price: dec("3"),
                    net: dec("3"),
                    rate: dec("0"),
                },
            ],
        }
    }

    #[test]
    fn invoice_totals_are_consistent() {
        let x = render(&doc("invoice"), &cfg(), &buyer());
        assert!(x.contains(CIUS_RO_CUSTOMIZATION));
        assert!(x.contains("<cbc:InvoiceTypeCode>380</cbc:InvoiceTypeCode>"));
        // 10.01 + 10.00 = 20.01 @19% = 3.80; 3.00 exempt
        assert!(x.contains("<cbc:LineExtensionAmount currencyID=\"EUR\">23.01</cbc:LineExtensionAmount><cbc:TaxExclusiveAmount"));
        assert!(
            x.contains("<cbc:TaxAmount currencyID=\"EUR\">3.80</cbc:TaxAmount><cac:TaxSubtotal>")
        );
        assert!(x.contains("<cbc:PayableAmount currencyID=\"EUR\">26.81</cbc:PayableAmount>"));
        assert!(
            x.contains("<cbc:ID>E</cbc:ID><cbc:Percent>0</cbc:Percent><cbc:TaxExemptionReason>")
        );
        assert!(x.contains("Mythical &amp; Co SRL") && x.contains("Acme &lt;GmbH&gt;"));
        assert!(x.contains("<cbc:CountrySubentity>RO-B</cbc:CountrySubentity>"));
        assert!(x.contains("<cbc:ID>RO49AAAA1B31007593840000</cbc:ID>"));
        assert!(!x.contains("<cbc:PrepaidAmount"));
    }

    #[test]
    fn credit_and_debit_notes() {
        let mut d = doc("credit_note");
        d.billing_reference = Some("INV-000001".into());
        let x = render(&d, &cfg(), &buyer());
        assert!(x.contains(
            "<CreditNote xmlns=\"urn:oasis:names:specification:ubl:schema:xsd:CreditNote-2\""
        ));
        assert!(x.contains("<cbc:CreditNoteTypeCode>381</cbc:CreditNoteTypeCode>"));
        assert!(x.contains("<cac:CreditNoteLine>") && x.contains("<cbc:CreditedQuantity"));
        assert!(x.contains("<cac:InvoiceDocumentReference><cbc:ID>INV-000001</cbc:ID>"));
        assert!(!x.contains("PaymentMeans") && !x.contains("DueDate"));
        let x = render(&doc("debit_note"), &cfg(), &buyer());
        assert!(x.contains("<cbc:InvoiceTypeCode>383</cbc:InvoiceTypeCode>"));
    }

    #[test]
    fn reverse_charge_and_prepaid() {
        let mut d = doc("invoice");
        d.reverse_charge = true;
        d.prepaid = dec("5");
        let x = render(&d, &cfg(), &buyer());
        assert!(x.contains(
            "<cbc:ID>AE</cbc:ID><cbc:Percent>0</cbc:Percent><cbc:TaxExemptionReason>Reverse charge"
        ));
        assert!(x.contains("<cbc:PrepaidAmount currencyID=\"EUR\">5.00</cbc:PrepaidAmount><cbc:PayableAmount currencyID=\"EUR\">18.01</cbc:PayableAmount>"));
    }

    #[test]
    fn settings_validation_and_missing() {
        assert!(Settings {
            profile: "x".into(),
            ..Default::default()
        }
        .normalized()
        .is_err());
        assert!(Settings {
            endpoint_scheme: "88".into(),
            ..Default::default()
        }
        .normalized()
        .is_err());
        let s = Settings::default().normalized().unwrap();
        assert_eq!(
            (s.profile.as_str(), s.payment_means_code.as_str()),
            ("peppol", "30")
        );
        assert_eq!(missing(&s, &Party::default()).len(), 4);
        assert!(missing(&cfg(), &buyer()).is_empty());
        let mut c = cfg();
        c.seller_subdivision.clear();
        assert_eq!(
            missing(&c, &buyer()),
            vec!["einvoice.missing.seller_subdivision"]
        );
    }
}
