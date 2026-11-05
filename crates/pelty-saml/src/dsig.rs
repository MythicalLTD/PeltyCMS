//! XML-DSig verification for enveloped signatures, restricted to a safe
//! profile: one Reference to the parent element by ID, Exclusive C14N 1.0,
//! SHA-256 digest, RSA-SHA256 or ECDSA-P256-SHA256. The key always comes from
//! the configured IdP certificate; KeyInfo in the message is ignored.
use crate::c14n::{canonicalize, EXC_C14N};
use crate::xml::Doc;
use crate::Error;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use der::{Decode, Encode};
use sha2::{Digest, Sha256};

pub const DS: &str = "http://www.w3.org/2000/09/xmldsig#";
pub const ENVELOPED: &str = "http://www.w3.org/2000/09/xmldsig#enveloped-signature";
pub const RSA_SHA256: &str = "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256";
pub const ECDSA_SHA256: &str = "http://www.w3.org/2001/04/xmldsig-more#ecdsa-sha256";
pub const SHA256: &str = "http://www.w3.org/2001/04/xmlenc#sha256";
const OID_RSA: &str = "1.2.840.113549.1.1.1";
const OID_EC: &str = "1.2.840.10045.2.1";
const OID_P256: &str = "1.2.840.10045.3.1.7";

#[derive(Debug, Clone)]
pub enum PublicKey {
    Rsa(rsa::RsaPublicKey),
    EcP256(Vec<u8>),
}

pub fn b64(s: &str) -> Result<Vec<u8>, Error> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    STANDARD
        .decode(clean)
        .map_err(|_| Error::Signature("base64"))
}

/// Normalize a certificate given as PEM or bare base64 into DER.
pub fn cert_der(input: &str) -> Result<Vec<u8>, Error> {
    let body: String = input
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<Vec<_>>()
        .join("");
    let der = b64(&body).map_err(|_| Error::Cert("encoding"))?;
    x509_cert::Certificate::from_der(&der).map_err(|_| Error::Cert("parse"))?;
    Ok(der)
}

/// PEM-wrap a DER certificate.
pub fn cert_pem(der: &[u8]) -> String {
    let b = STANDARD.encode(der);
    let lines: Vec<&str> = b
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();
    format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
        lines.join("\n")
    )
}

impl PublicKey {
    pub fn from_cert(input: &str) -> Result<PublicKey, Error> {
        let der = cert_der(input)?;
        let cert = x509_cert::Certificate::from_der(&der).map_err(|_| Error::Cert("parse"))?;
        let spki = cert.tbs_certificate().subject_public_key_info();
        let bits = spki.subject_public_key.raw_bytes();
        match spki.algorithm.oid.to_string().as_str() {
            OID_RSA => {
                use rsa::pkcs1::DecodeRsaPublicKey;
                use rsa::traits::PublicKeyParts;
                let k = rsa::RsaPublicKey::from_pkcs1_der(bits).map_err(|_| Error::Cert("rsa"))?;
                if k.n().bits() < 2048 {
                    return Err(Error::Cert("rsa_too_small"));
                }
                Ok(PublicKey::Rsa(k))
            }
            OID_EC => {
                let curve = spki.algorithm.parameters.as_ref().and_then(|p| {
                    let encoded = p.to_der().ok()?;
                    let oid: der::asn1::ObjectIdentifier = Decode::from_der(&encoded).ok()?;
                    Some(oid.to_string())
                });
                if curve.as_deref() != Some(OID_P256) {
                    return Err(Error::Cert("ec_curve"));
                }
                Ok(PublicKey::EcP256(bits.to_vec()))
            }
            _ => Err(Error::Cert("algorithm")),
        }
    }

    /// Verify a raw signature for an algorithm URI.
    pub fn verify(&self, alg: &str, msg: &[u8], sig: &[u8]) -> Result<(), Error> {
        match (self, alg) {
            (PublicKey::Rsa(k), RSA_SHA256) => {
                use rsa::signature::Verifier;
                let vk = rsa::pkcs1v15::VerifyingKey::<Sha256>::new(k.clone());
                let s = rsa::pkcs1v15::Signature::try_from(sig)
                    .map_err(|_| Error::Signature("value"))?;
                vk.verify(msg, &s).map_err(|_| Error::Signature("invalid"))
            }
            (PublicKey::EcP256(p), ECDSA_SHA256) => ring::signature::UnparsedPublicKey::new(
                &ring::signature::ECDSA_P256_SHA256_FIXED,
                p,
            )
            .verify(msg, sig)
            .map_err(|_| Error::Signature("invalid")),
            _ => Err(Error::Signature("algorithm")),
        }
    }
}

/// Every `ID` attribute value must be unique in the document; duplicates
/// are the classic signature-wrapping trick.
pub fn ids_unique(doc: &Doc) -> Result<(), Error> {
    let mut seen = std::collections::HashSet::new();
    for i in doc.all() {
        if let Some(id) = doc.attr(i, "ID") {
            if !seen.insert(id.to_string()) {
                return Err(Error::Signature("duplicate_id"));
            }
        }
    }
    Ok(())
}

fn prefix_list(doc: &Doc, transform: usize) -> Vec<String> {
    doc.kids_named(transform, EXC_C14N, "InclusiveNamespaces")
        .next()
        .and_then(|n| doc.attr(n, "PrefixList"))
        .map(|p| p.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Verify the enveloped `ds:Signature` at `sig` against `keys`.
/// On success returns the index of the signed element, which is always the
/// signature's parent, i.e. the exact node the caller must consume.
pub fn verify_enveloped(doc: &Doc, sig: usize, keys: &[PublicKey]) -> Result<usize, Error> {
    let bad = Error::Signature;
    if !doc.is(sig, DS, "Signature") {
        return Err(bad("not_signature"));
    }
    let signed = doc.e(sig).parent.ok_or(bad("no_parent"))?;
    let id = doc
        .attr(signed, "ID")
        .filter(|v| !v.is_empty())
        .ok_or(bad("no_id"))?;
    let si = doc.only(sig, DS, "SignedInfo").ok_or(bad("signed_info"))?;
    let cm = doc
        .only(si, DS, "CanonicalizationMethod")
        .ok_or(bad("c14n"))?;
    if doc.attr(cm, "Algorithm") != Some(EXC_C14N) {
        return Err(bad("c14n_algorithm"));
    }
    let sm = doc.only(si, DS, "SignatureMethod").ok_or(bad("method"))?;
    let alg = doc.attr(sm, "Algorithm").unwrap_or("");
    if alg != RSA_SHA256 && alg != ECDSA_SHA256 {
        return Err(bad("algorithm"));
    }
    let r = doc
        .only(si, DS, "Reference")
        .ok_or(bad("reference_count"))?;
    if doc.attr(r, "URI") != Some(&format!("#{id}")) {
        return Err(bad("reference_uri"));
    }
    let mut inclusive = vec![];
    let mut env = 0;
    let mut exc = 0;
    if let Some(ts) = doc.only(r, DS, "Transforms") {
        for t in doc.kids(ts) {
            if !doc.is(t, DS, "Transform") {
                return Err(bad("transform"));
            }
            match doc.attr(t, "Algorithm") {
                Some(ENVELOPED) => env += 1,
                Some(EXC_C14N) => {
                    exc += 1;
                    inclusive = prefix_list(doc, t);
                }
                _ => return Err(bad("transform")),
            }
        }
    }
    if env != 1 || exc > 1 {
        return Err(bad("transform"));
    }
    let dm = doc.only(r, DS, "DigestMethod").ok_or(bad("digest"))?;
    if doc.attr(dm, "Algorithm") != Some(SHA256) {
        return Err(bad("digest_algorithm"));
    }
    let dv = doc.only(r, DS, "DigestValue").ok_or(bad("digest"))?;
    let expected = b64(&doc.text(dv))?;
    let canon = canonicalize(doc, signed, Some(sig), &inclusive);
    if Sha256::digest(canon.as_bytes()).as_slice() != expected.as_slice() {
        return Err(bad("digest_mismatch"));
    }
    let sv = doc.only(sig, DS, "SignatureValue").ok_or(bad("value"))?;
    let value = b64(&doc.text(sv))?;
    let si_canon = canonicalize(doc, si, None, &prefix_list(doc, cm));
    if keys
        .iter()
        .any(|k| k.verify(alg, si_canon.as_bytes(), &value).is_ok())
    {
        Ok(signed)
    } else {
        Err(bad("invalid"))
    }
}
