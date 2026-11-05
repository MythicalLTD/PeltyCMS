//! SAML 2.0 Service Provider primitives, pure Rust, no `unsafe`, no C.
//!
//! Scope (deliberately narrow): Exclusive C14N 1.0, enveloped XML-DSig with
//! RSA-SHA256 / ECDSA-P256-SHA256 and SHA-256 digests, pinned IdP
//! certificates, HTTP-POST responses and HTTP-Redirect requests. Not
//! supported: EncryptedAssertion, SHA-1, inclusive C14N, XPath transforms,
//! KeyInfo-supplied keys, artifact binding.
pub mod c14n;
pub mod dsig;
pub mod keys;
pub mod sp;
pub mod testing;
pub mod xml;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Xml(&'static str),
    Signature(&'static str),
    Cert(&'static str),
    Invalid(&'static str),
}

impl Error {
    /// Stable short code for logs and the admin test page.
    pub fn code(&self) -> String {
        match self {
            Error::Xml(c) => format!("xml.{c}"),
            Error::Signature(c) => format!("signature.{c}"),
            Error::Cert(c) => format!("cert.{c}"),
            Error::Invalid(c) => format!("invalid.{c}"),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.code())
    }
}
impl std::error::Error for Error {}
