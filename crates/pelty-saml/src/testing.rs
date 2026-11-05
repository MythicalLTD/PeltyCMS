//! A simulated Identity Provider for tests and the admin "test" button
//! fixtures: builds and signs SAML Responses with locally generated keys.
//! Not used on any production code path.
use crate::c14n::{canonicalize, EXC_C14N};
use crate::dsig::{DS, ENVELOPED, SHA256};
use crate::keys::SigningKey;
use crate::sp::{esc, now_str, ASSERT, PROTO, SUCCESS};
use crate::xml::Doc;
use crate::Error;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};

/// Replace the comment `<!--SIG:{id}-->` with an enveloped signature over the
/// element whose ID is `id`. Comments are dropped by C14N, so the digest of
/// the marked document equals the digest after insertion.
pub fn sign_enveloped(xml: &str, id: &str, key: &SigningKey) -> Result<String, Error> {
    let marker = format!("<!--SIG:{id}-->");
    if !xml.contains(&marker) {
        return Err(Error::Invalid("marker"));
    }
    let d = Doc::parse(xml)?;
    let el = d
        .all()
        .find(|&i| d.attr(i, "ID") == Some(id))
        .ok_or(Error::Invalid("id"))?;
    let digest = STANDARD.encode(Sha256::digest(canonicalize(&d, el, None, &[]).as_bytes()));
    let si = format!(
        "<ds:SignedInfo><ds:CanonicalizationMethod Algorithm=\"{EXC_C14N}\"/><ds:SignatureMethod Algorithm=\"{}\"/><ds:Reference URI=\"#{id}\"><ds:Transforms><ds:Transform Algorithm=\"{ENVELOPED}\"/><ds:Transform Algorithm=\"{EXC_C14N}\"/></ds:Transforms><ds:DigestMethod Algorithm=\"{SHA256}\"/><ds:DigestValue>{digest}</ds:DigestValue></ds:Reference></ds:SignedInfo>",
        key.alg()
    );
    let frag = format!("<ds:Signature xmlns:ds=\"{DS}\">{si}</ds:Signature>");
    let fd = Doc::parse(&frag)?;
    let si_idx = fd.kids(fd.root).next().ok_or(Error::Invalid("frag"))?;
    let sig = STANDARD.encode(key.sign(canonicalize(&fd, si_idx, None, &[]).as_bytes())?);
    let full = format!("<ds:Signature xmlns:ds=\"{DS}\">{si}<ds:SignatureValue>{sig}</ds:SignatureValue></ds:Signature>");
    Ok(xml.replacen(&marker, &full, 1))
}

pub struct TestIdp {
    pub entity_id: String,
    pub key: SigningKey,
    pub cert_pem: String,
}

/// Knobs for building a response, defaults describe a valid one.
pub struct ResponseSpec {
    pub response_id: String,
    pub assertion_id: String,
    pub in_response_to: Option<String>,
    pub destination: String,
    pub audience: String,
    pub recipient: String,
    pub issuer: String,
    pub name_id: String,
    pub attributes: Vec<(String, Vec<String>)>,
    pub issued: OffsetDateTime,
    pub lifetime: Duration,
    pub sign_assertion: bool,
    pub sign_response: bool,
}

impl ResponseSpec {
    pub fn new(
        idp: &str,
        sp_entity: &str,
        acs: &str,
        name_id: &str,
        in_response_to: Option<&str>,
    ) -> Self {
        ResponseSpec {
            response_id: crate::sp::new_id(),
            assertion_id: crate::sp::new_id(),
            in_response_to: in_response_to.map(str::to_string),
            destination: acs.into(),
            audience: sp_entity.into(),
            recipient: acs.into(),
            issuer: idp.into(),
            name_id: name_id.into(),
            attributes: vec![],
            issued: OffsetDateTime::now_utc(),
            lifetime: Duration::minutes(5),
            sign_assertion: true,
            sign_response: false,
        }
    }
}

impl TestIdp {
    pub fn new(entity_id: &str, ec: bool) -> Result<TestIdp, Error> {
        let key = if ec {
            SigningKey::generate_ec()?
        } else {
            SigningKey::generate_rsa()?
        };
        let cert_pem = key.certificate("test-idp", 30)?;
        Ok(TestIdp {
            entity_id: entity_id.into(),
            key,
            cert_pem,
        })
    }

    /// Unsigned assertion XML with a signature marker (for wrapping tests).
    pub fn assertion_xml(&self, r: &ResponseSpec) -> String {
        let irt = r
            .in_response_to
            .as_ref()
            .map(|v| format!(" InResponseTo=\"{}\"", esc(v)))
            .unwrap_or_default();
        let attrs: String = r
            .attributes
            .iter()
            .map(|(n, vs)| {
                let v: String = vs
                    .iter()
                    .map(|v| format!("<saml:AttributeValue>{}</saml:AttributeValue>", esc(v)))
                    .collect();
                format!("<saml:Attribute Name=\"{}\">{v}</saml:Attribute>", esc(n))
            })
            .collect();
        let issued = now_str(r.issued);
        let until = now_str(r.issued + r.lifetime);
        format!(
            "<saml:Assertion xmlns:saml=\"{ASSERT}\" ID=\"{aid}\" Version=\"2.0\" IssueInstant=\"{issued}\"><saml:Issuer>{iss}</saml:Issuer><!--SIG:{aid}--><saml:Subject><saml:NameID Format=\"urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress\">{nid}</saml:NameID><saml:SubjectConfirmation Method=\"urn:oasis:names:tc:SAML:2.0:cm:bearer\"><saml:SubjectConfirmationData NotOnOrAfter=\"{until}\" Recipient=\"{rcp}\"{irt}/></saml:SubjectConfirmation></saml:Subject><saml:Conditions NotBefore=\"{issued}\" NotOnOrAfter=\"{until}\"><saml:AudienceRestriction><saml:Audience>{aud}</saml:Audience></saml:AudienceRestriction></saml:Conditions><saml:AuthnStatement AuthnInstant=\"{issued}\" SessionIndex=\"sess-{aid}\"><saml:AuthnContext><saml:AuthnContextClassRef>urn:oasis:names:tc:SAML:2.0:ac:classes:Password</saml:AuthnContextClassRef></saml:AuthnContext></saml:AuthnStatement><saml:AttributeStatement>{attrs}</saml:AttributeStatement></saml:Assertion>",
            aid = esc(&r.assertion_id),
            iss = esc(&r.issuer),
            nid = esc(&r.name_id),
            rcp = esc(&r.recipient),
            aud = esc(&r.audience),
        )
    }

    pub fn response(&self, r: &ResponseSpec) -> Result<String, Error> {
        let mut a = self.assertion_xml(r);
        if r.sign_assertion {
            a = sign_enveloped(&a, &r.assertion_id, &self.key)?;
        } else {
            a = a.replace(&format!("<!--SIG:{}-->", r.assertion_id), "");
        }
        self.wrap(r, &a)
    }

    /// Put arbitrary assertion XML inside a Response envelope.
    pub fn wrap(&self, r: &ResponseSpec, assertion: &str) -> Result<String, Error> {
        let irt = r
            .in_response_to
            .as_ref()
            .map(|v| format!(" InResponseTo=\"{}\"", esc(v)))
            .unwrap_or_default();
        let xml = format!(
            "<samlp:Response xmlns:samlp=\"{PROTO}\" xmlns:saml=\"{ASSERT}\" ID=\"{rid}\" Version=\"2.0\" IssueInstant=\"{}\" Destination=\"{}\"{irt}><saml:Issuer>{}</saml:Issuer><!--SIG:{rid}--><samlp:Status><samlp:StatusCode Value=\"{SUCCESS}\"/></samlp:Status>{assertion}</samlp:Response>",
            now_str(r.issued),
            esc(&r.destination),
            esc(&r.issuer),
            rid = esc(&r.response_id),
        );
        if r.sign_response {
            sign_enveloped(&xml, &r.response_id, &self.key)
        } else {
            Ok(xml.replace(&format!("<!--SIG:{}-->", r.response_id), ""))
        }
    }

    pub fn metadata(&self, sso_url: &str, slo_url: &str) -> String {
        let b: String = self
            .cert_pem
            .lines()
            .filter(|l| !l.starts_with("-----"))
            .collect();
        format!(
            "<md:EntityDescriptor xmlns:md=\"urn:oasis:names:tc:SAML:2.0:metadata\" entityID=\"{}\"><md:IDPSSODescriptor protocolSupportEnumeration=\"{PROTO}\"><md:KeyDescriptor use=\"signing\"><ds:KeyInfo xmlns:ds=\"{DS}\"><ds:X509Data><ds:X509Certificate>{b}</ds:X509Certificate></ds:X509Data></ds:KeyInfo></md:KeyDescriptor><md:SingleLogoutService Binding=\"{}\" Location=\"{}\"/><md:SingleSignOnService Binding=\"{}\" Location=\"{}\"/></md:IDPSSODescriptor></md:EntityDescriptor>",
            esc(&self.entity_id),
            crate::sp::BIND_REDIRECT,
            esc(slo_url),
            crate::sp::BIND_REDIRECT,
            esc(sso_url)
        )
    }
}
