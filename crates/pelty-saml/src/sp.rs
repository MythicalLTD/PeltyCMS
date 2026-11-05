//! SAML 2.0 Web Browser SSO profile, Service Provider side.
use crate::dsig::{self, PublicKey, DS};
use crate::keys::SigningKey;
use crate::xml::Doc;
use crate::Error;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

pub const PROTO: &str = "urn:oasis:names:tc:SAML:2.0:protocol";
pub const ASSERT: &str = "urn:oasis:names:tc:SAML:2.0:assertion";
pub const MD: &str = "urn:oasis:names:tc:SAML:2.0:metadata";
pub const BIND_REDIRECT: &str = "urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect";
pub const BIND_POST: &str = "urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST";
pub const SUCCESS: &str = "urn:oasis:names:tc:SAML:2.0:status:Success";
const BEARER: &str = "urn:oasis:names:tc:SAML:2.0:cm:bearer";

pub struct Sp<'a> {
    pub entity_id: &'a str,
    pub acs_url: &'a str,
    pub slo_url: &'a str,
}

pub struct Idp<'a> {
    pub entity_id: &'a str,
    pub keys: &'a [PublicKey],
}

pub struct Expect<'a> {
    /// ID of our AuthnRequest; `None` = IdP-initiated (unsolicited).
    pub request_id: Option<&'a str>,
    pub allow_unsolicited: bool,
    pub require_signed_assertion: bool,
    pub now: OffsetDateTime,
    pub skew: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assertion {
    pub id: String,
    pub issuer: String,
    pub name_id: String,
    pub name_id_format: String,
    pub session_index: Option<String>,
    pub attributes: BTreeMap<String, Vec<String>>,
    /// The assertion ID must be remembered (replay cache) until this instant.
    pub replay_until: OffsetDateTime,
    pub in_response_to: Option<String>,
}

impl Assertion {
    /// First value of the first attribute matching `name` (by Name or FriendlyName).
    pub fn first(&self, name: &str) -> Option<&str> {
        if name.is_empty() {
            return None;
        }
        self.attributes
            .get(name)
            .and_then(|v| v.first())
            .map(String::as_str)
    }
    pub fn all(&self, name: &str) -> &[String] {
        self.attributes.get(name).map(Vec::as_slice).unwrap_or(&[])
    }
}

fn ts(s: &str) -> Result<OffsetDateTime, Error> {
    OffsetDateTime::parse(s.trim(), &Rfc3339).map_err(|_| Error::Invalid("timestamp"))
}

pub fn now_str(t: OffsetDateTime) -> String {
    let t = t
        .to_offset(time::UtcOffset::UTC)
        .replace_nanosecond(0)
        .unwrap_or(t);
    t.format(&Rfc3339).unwrap_or_default()
}

pub fn esc(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}

pub fn new_id() -> String {
    format!("_{}", uuid::Uuid::new_v4().simple())
}

/// The `InResponseTo` of a Response, read before validation so the caller can
/// look up its pending request. Never trust anything else from this peek.
pub fn peek_in_response_to(xml: &str) -> Option<String> {
    let d = Doc::parse(xml).ok()?;
    d.attr(d.root, "InResponseTo").map(str::to_string)
}

/// Decode the base64 `SAMLResponse` of the HTTP-POST binding.
pub fn decode_post(b64: &str) -> Result<String, Error> {
    let raw = dsig::b64(b64).map_err(|_| Error::Invalid("encoding"))?;
    if raw.len() > crate::xml::MAX_INPUT {
        return Err(Error::Xml("too_large"));
    }
    String::from_utf8(raw).map_err(|_| Error::Invalid("encoding"))
}

/// Validate a `samlp:Response` and return the one assertion it carries.
pub fn validate_response(xml: &str, sp: &Sp, idp: &Idp, ex: &Expect) -> Result<Assertion, Error> {
    let inv = Error::Invalid;
    let d = Doc::parse(xml)?;
    let root = d.root;
    if !d.is(root, PROTO, "Response") || d.attr(root, "Version") != Some("2.0") {
        return Err(inv("not_response"));
    }
    dsig::ids_unique(&d)?;
    if d.all().any(|i| d.is(i, ASSERT, "EncryptedAssertion")) {
        return Err(inv("encrypted_assertion_unsupported"));
    }
    // Exactly one assertion in the whole document, as a direct child of the
    // Response: nothing hidden elsewhere for a confused consumer to pick up.
    let assertions: Vec<usize> = d.all().filter(|&i| d.is(i, ASSERT, "Assertion")).collect();
    let [a] = assertions[..] else {
        return Err(inv("assertion_count"));
    };
    if d.e(a).parent != Some(root) || d.attr(a, "Version") != Some("2.0") {
        return Err(inv("assertion_position"));
    }
    // Signatures may only sit directly in the Response or the Assertion.
    let sigs: Vec<usize> = d.all().filter(|&i| d.is(i, DS, "Signature")).collect();
    let mut resp_signed = false;
    let mut assertion_signed = false;
    for &s in &sigs {
        let p = d.e(s).parent;
        let signed = dsig::verify_enveloped(&d, s, idp.keys)?;
        if p == Some(root) && signed == root && !resp_signed {
            resp_signed = true;
        } else if p == Some(a) && signed == a && !assertion_signed {
            assertion_signed = true;
        } else {
            return Err(inv("signature_position"));
        }
    }
    if !(assertion_signed || resp_signed && !ex.require_signed_assertion) {
        return Err(Error::Signature("missing"));
    }

    let now = ex.now;
    let skew = ex.skew;
    if let Some(dest) = d.attr(root, "Destination") {
        if dest != sp.acs_url {
            return Err(inv("destination"));
        }
    }
    let status = d
        .only(root, PROTO, "Status")
        .and_then(|s| d.only(s, PROTO, "StatusCode"))
        .ok_or(inv("status"))?;
    if d.attr(status, "Value") != Some(SUCCESS) {
        return Err(inv("status_not_success"));
    }
    if let Some(i) = d.only(root, ASSERT, "Issuer") {
        if d.text(i).trim() != idp.entity_id {
            return Err(inv("issuer"));
        }
    }
    let irt = d.attr(root, "InResponseTo").map(str::to_string);
    match (ex.request_id, irt.as_deref()) {
        (Some(want), Some(got)) if want == got => {}
        (None, None) if ex.allow_unsolicited => {}
        (None, None) => return Err(inv("unsolicited")),
        _ => return Err(inv("in_response_to")),
    }
    if let Some(t) = d.attr(root, "IssueInstant") {
        if ts(t)? > now + skew {
            return Err(inv("issued_in_future"));
        }
    }

    // Everything below reads only from `a`, the verified node.
    let issuer = d
        .only(a, ASSERT, "Issuer")
        .map(|i| d.text(i).trim().to_string())
        .ok_or(inv("issuer"))?;
    if issuer != idp.entity_id {
        return Err(inv("issuer"));
    }
    let id = d.attr(a, "ID").ok_or(inv("assertion_id"))?.to_string();
    let subject = d.only(a, ASSERT, "Subject").ok_or(inv("subject"))?;
    let nid = d.only(subject, ASSERT, "NameID").ok_or(inv("name_id"))?;
    let name_id = d.text(nid).trim().to_string();
    if name_id.is_empty() || name_id.len() > 255 {
        return Err(inv("name_id"));
    }
    let mut replay_until = now;
    let mut confirmed = false;
    for sc in d.kids_named(subject, ASSERT, "SubjectConfirmation") {
        if d.attr(sc, "Method") != Some(BEARER) {
            continue;
        }
        let Some(scd) = d.only(sc, ASSERT, "SubjectConfirmationData") else {
            continue;
        };
        let Some(noa) = d.attr(scd, "NotOnOrAfter") else {
            continue;
        };
        let noa = ts(noa)?;
        if now >= noa + skew {
            continue;
        }
        if let Some(nb) = d.attr(scd, "NotBefore") {
            if now + skew < ts(nb)? {
                continue;
            }
        }
        if d.attr(scd, "Recipient") != Some(sp.acs_url) {
            continue;
        }
        if d.attr(scd, "InResponseTo") != ex.request_id {
            continue;
        }
        confirmed = true;
        replay_until = replay_until.max(noa + skew);
    }
    if !confirmed {
        return Err(inv("subject_confirmation"));
    }
    let cond = d.only(a, ASSERT, "Conditions").ok_or(inv("conditions"))?;
    if let Some(nb) = d.attr(cond, "NotBefore") {
        if now + skew < ts(nb)? {
            return Err(inv("not_yet_valid"));
        }
    }
    if let Some(noa) = d.attr(cond, "NotOnOrAfter") {
        let noa = ts(noa)?;
        if now >= noa + skew {
            return Err(inv("expired"));
        }
        replay_until = replay_until.max(noa + skew);
    }
    let mut restrictions = 0;
    for ar in d.kids_named(cond, ASSERT, "AudienceRestriction") {
        restrictions += 1;
        if !d
            .kids_named(ar, ASSERT, "Audience")
            .any(|x| d.text(x).trim() == sp.entity_id)
        {
            return Err(inv("audience"));
        }
    }
    if restrictions == 0 {
        return Err(inv("audience"));
    }
    let mut session_index = None;
    for st in d.kids_named(a, ASSERT, "AuthnStatement") {
        if let Some(sno) = d.attr(st, "SessionNotOnOrAfter") {
            if now >= ts(sno)? + skew {
                return Err(inv("session_expired"));
            }
        }
        session_index = d.attr(st, "SessionIndex").map(str::to_string);
    }
    let mut attributes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for st in d.kids_named(a, ASSERT, "AttributeStatement") {
        for at in d.kids_named(st, ASSERT, "Attribute") {
            let vals: Vec<String> = d
                .kids_named(at, ASSERT, "AttributeValue")
                .map(|v| d.text(v).trim().to_string())
                .filter(|v| !v.is_empty())
                .collect();
            for key in [d.attr(at, "Name"), d.attr(at, "FriendlyName")]
                .into_iter()
                .flatten()
            {
                attributes
                    .entry(key.to_string())
                    .or_default()
                    .extend(vals.iter().cloned());
            }
        }
    }
    Ok(Assertion {
        id,
        issuer,
        name_id,
        name_id_format: d.attr(nid, "Format").unwrap_or("").to_string(),
        session_index,
        attributes,
        replay_until,
        in_response_to: irt,
    })
}

fn deflate(xml: &str) -> Result<String, Error> {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(xml.as_bytes())
        .map_err(|_| Error::Invalid("deflate"))?;
    Ok(STANDARD.encode(e.finish().map_err(|_| Error::Invalid("deflate"))?))
}

/// Inflate an HTTP-Redirect binding message (bounded against zip bombs).
pub fn inflate(b64: &str) -> Result<String, Error> {
    let raw = dsig::b64(b64).map_err(|_| Error::Invalid("encoding"))?;
    let mut out = String::new();
    flate2::read::DeflateDecoder::new(&raw[..])
        .take(crate::xml::MAX_INPUT as u64 + 1)
        .read_to_string(&mut out)
        .map_err(|_| Error::Invalid("inflate"))?;
    if out.len() > crate::xml::MAX_INPUT {
        return Err(Error::Xml("too_large"));
    }
    Ok(out)
}

fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Build an HTTP-Redirect URL: `param` is `SAMLRequest` or `SAMLResponse`.
/// With a key, the query is signed per the binding spec (section 3.4.4.1).
pub fn redirect_url(
    endpoint: &str,
    param: &str,
    xml: &str,
    relay: Option<&str>,
    key: Option<&SigningKey>,
) -> Result<String, Error> {
    let mut q = format!("{param}={}", enc(&deflate(xml)?));
    if let Some(r) = relay {
        q.push_str(&format!("&RelayState={}", enc(r)));
    }
    if let Some(k) = key {
        q.push_str(&format!("&SigAlg={}", enc(k.alg())));
        let sig = STANDARD.encode(k.sign(q.as_bytes())?);
        q.push_str(&format!("&Signature={}", enc(&sig)));
    }
    let sep = if endpoint.contains('?') { '&' } else { '?' };
    Ok(format!("{endpoint}{sep}{q}"))
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |x: u8| (x as char).to_digit(16);
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => match (hex(b[i + 1]), hex(b[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push((h * 16 + l) as u8);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A parsed HTTP-Redirect query (raw values kept for signature checks).
pub struct RedirectMsg {
    pub param: String,
    pub xml: String,
    pub relay: Option<String>,
    pub signed: bool,
}

/// Parse an HTTP-Redirect query and verify its detached signature if `keys`
/// is non-empty (then a signature is mandatory).
pub fn parse_redirect(raw_query: &str, keys: &[PublicKey]) -> Result<RedirectMsg, Error> {
    let mut parts: BTreeMap<&str, &str> = BTreeMap::new();
    for kv in raw_query.split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        if parts.insert(k, v).is_some() {
            return Err(Error::Invalid("duplicate_param"));
        }
    }
    let (param, val) = if let Some(v) = parts.get("SAMLRequest") {
        ("SAMLRequest", *v)
    } else {
        (
            "SAMLResponse",
            *parts
                .get("SAMLResponse")
                .ok_or(Error::Invalid("missing_message"))?,
        )
    };
    let mut signed = false;
    if !keys.is_empty() {
        let alg = pct_decode(parts.get("SigAlg").ok_or(Error::Signature("missing"))?);
        let sig = dsig::b64(&pct_decode(
            parts.get("Signature").ok_or(Error::Signature("missing"))?,
        ))?;
        let mut msg = format!("{param}={val}");
        if let Some(r) = parts.get("RelayState") {
            msg.push_str(&format!("&RelayState={r}"));
        }
        msg.push_str(&format!("&SigAlg={}", parts["SigAlg"]));
        if !keys
            .iter()
            .any(|k| k.verify(&alg, msg.as_bytes(), &sig).is_ok())
        {
            return Err(Error::Signature("invalid"));
        }
        signed = true;
    }
    Ok(RedirectMsg {
        param: param.into(),
        xml: inflate(&pct_decode(val))?,
        relay: parts.get("RelayState").map(|r| pct_decode(r)),
        signed,
    })
}

pub fn authn_request(id: &str, sp: &Sp, destination: &str, now: OffsetDateTime) -> String {
    format!(
        "<samlp:AuthnRequest xmlns:samlp=\"{PROTO}\" xmlns:saml=\"{ASSERT}\" ID=\"{}\" Version=\"2.0\" IssueInstant=\"{}\" Destination=\"{}\" AssertionConsumerServiceURL=\"{}\" ProtocolBinding=\"{BIND_POST}\"><saml:Issuer>{}</saml:Issuer><samlp:NameIDPolicy AllowCreate=\"true\"/></samlp:AuthnRequest>",
        esc(id), now_str(now), esc(destination), esc(sp.acs_url), esc(sp.entity_id)
    )
}

pub fn logout_request(
    id: &str,
    sp: &Sp,
    destination: &str,
    name_id: &str,
    session_index: Option<&str>,
    now: OffsetDateTime,
) -> String {
    let si = session_index
        .map(|s| format!("<samlp:SessionIndex>{}</samlp:SessionIndex>", esc(s)))
        .unwrap_or_default();
    format!(
        "<samlp:LogoutRequest xmlns:samlp=\"{PROTO}\" xmlns:saml=\"{ASSERT}\" ID=\"{}\" Version=\"2.0\" IssueInstant=\"{}\" Destination=\"{}\"><saml:Issuer>{}</saml:Issuer><saml:NameID>{}</saml:NameID>{si}</samlp:LogoutRequest>",
        esc(id), now_str(now), esc(destination), esc(sp.entity_id), esc(name_id)
    )
}

pub fn logout_response(
    in_response_to: &str,
    sp: &Sp,
    destination: &str,
    now: OffsetDateTime,
) -> String {
    format!(
        "<samlp:LogoutResponse xmlns:samlp=\"{PROTO}\" xmlns:saml=\"{ASSERT}\" ID=\"{}\" Version=\"2.0\" IssueInstant=\"{}\" Destination=\"{}\" InResponseTo=\"{}\"><saml:Issuer>{}</saml:Issuer><samlp:Status><samlp:StatusCode Value=\"{SUCCESS}\"/></samlp:Status></samlp:LogoutResponse>",
        new_id(), now_str(now), esc(destination), esc(in_response_to), esc(sp.entity_id)
    )
}

pub struct LogoutReq {
    pub id: String,
    pub issuer: String,
    pub name_id: String,
}

/// Parse an IdP `LogoutRequest` (signature already checked on the binding).
pub fn parse_logout_request(
    xml: &str,
    idp_entity: &str,
    now: OffsetDateTime,
    skew: Duration,
) -> Result<LogoutReq, Error> {
    let d = Doc::parse(xml)?;
    let r = d.root;
    if !d.is(r, PROTO, "LogoutRequest") {
        return Err(Error::Invalid("not_logout_request"));
    }
    let issuer = d
        .only(r, ASSERT, "Issuer")
        .map(|i| d.text(i).trim().to_string())
        .unwrap_or_default();
    if issuer != idp_entity {
        return Err(Error::Invalid("issuer"));
    }
    if let Some(noa) = d.attr(r, "NotOnOrAfter") {
        if now >= ts(noa)? + skew {
            return Err(Error::Invalid("expired"));
        }
    }
    let name_id = d
        .only(r, ASSERT, "NameID")
        .map(|n| d.text(n).trim().to_string())
        .ok_or(Error::Invalid("name_id"))?;
    Ok(LogoutReq {
        id: d.attr(r, "ID").unwrap_or("").to_string(),
        issuer,
        name_id,
    })
}

pub fn sp_metadata(
    sp: &Sp,
    signing_cert_pem: Option<&str>,
    want_assertions_signed: bool,
) -> String {
    let kd = signing_cert_pem
        .map(|pem| {
            let b: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
            format!("<md:KeyDescriptor use=\"signing\"><ds:KeyInfo xmlns:ds=\"{DS}\"><ds:X509Data><ds:X509Certificate>{b}</ds:X509Certificate></ds:X509Data></ds:KeyInfo></md:KeyDescriptor>")
        })
        .unwrap_or_default();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<md:EntityDescriptor xmlns:md=\"{MD}\" entityID=\"{}\"><md:SPSSODescriptor AuthnRequestsSigned=\"{}\" WantAssertionsSigned=\"{want_assertions_signed}\" protocolSupportEnumeration=\"{PROTO}\">{kd}<md:SingleLogoutService Binding=\"{BIND_REDIRECT}\" Location=\"{}\"/><md:NameIDFormat>urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress</md:NameIDFormat><md:AssertionConsumerService Binding=\"{BIND_POST}\" Location=\"{}\" index=\"0\" isDefault=\"true\"/></md:SPSSODescriptor></md:EntityDescriptor>\n",
        esc(sp.entity_id),
        signing_cert_pem.is_some(),
        esc(sp.slo_url),
        esc(sp.acs_url)
    )
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IdpMetadata {
    pub entity_id: String,
    pub sso_url: String,
    pub slo_url: String,
    pub certs: Vec<String>,
}

/// Extract the essentials from IdP metadata (EntityDescriptor or the first
/// IdP in an EntitiesDescriptor). Metadata signatures are not checked: the
/// admin supplies the source and reviews the certificate.
pub fn parse_idp_metadata(xml: &str) -> Result<IdpMetadata, Error> {
    let d = Doc::parse(xml)?;
    let ed = d
        .all()
        .find(|&i| {
            d.is(i, MD, "EntityDescriptor")
                && d.kids_named(i, MD, "IDPSSODescriptor").next().is_some()
        })
        .ok_or(Error::Invalid("no_idp"))?;
    let idp = d
        .kids_named(ed, MD, "IDPSSODescriptor")
        .next()
        .ok_or(Error::Invalid("no_idp"))?;
    let loc = |name: &str| {
        d.kids_named(idp, MD, name)
            .find(|&s| d.attr(s, "Binding") == Some(BIND_REDIRECT))
            .and_then(|s| d.attr(s, "Location"))
            .unwrap_or("")
            .to_string()
    };
    let mut certs = vec![];
    for kd in d.kids_named(idp, MD, "KeyDescriptor") {
        if matches!(d.attr(kd, "use"), Some(u) if u != "signing") {
            continue;
        }
        for c in d
            .all()
            .filter(|&c| d.is(c, DS, "X509Certificate") && d.is_descendant(c, kd))
        {
            let t: String = d.text(c).chars().filter(|c| !c.is_whitespace()).collect();
            if !t.is_empty() && !certs.contains(&t) {
                certs.push(t);
            }
        }
    }
    Ok(IdpMetadata {
        entity_id: d.attr(ed, "entityID").unwrap_or("").to_string(),
        sso_url: loc("SingleSignOnService"),
        slo_url: loc("SingleLogoutService"),
        certs,
    })
}
