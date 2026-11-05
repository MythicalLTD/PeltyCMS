//! Signing keys and self-signed certificates (SP request signing, and the
//! simulated IdP used by tests).
use crate::dsig::{cert_pem, ECDSA_SHA256, RSA_SHA256};
use crate::Error;
use der::{Decode, Encode};
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey};
use sha2::Sha256;
use std::str::FromStr;
use x509_cert::builder::{Builder, CertificateBuilder, Profile};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::SubjectPublicKeyInfoOwned;
use x509_cert::time::Validity;

pub enum SigningKey {
    Rsa(Box<rsa::RsaPrivateKey>),
    EcP256(Box<ring::signature::EcdsaKeyPair>),
}

impl SigningKey {
    pub fn generate_rsa() -> Result<SigningKey, Error> {
        let k = rsa::RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048)
            .map_err(|_| Error::Cert("keygen"))?;
        Ok(SigningKey::Rsa(Box::new(k)))
    }

    pub fn generate_ec() -> Result<SigningKey, Error> {
        let rng = ring::rand::SystemRandom::new();
        let alg = &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING;
        let doc = ring::signature::EcdsaKeyPair::generate_pkcs8(alg, &rng)
            .map_err(|_| Error::Cert("keygen"))?;
        let kp = ring::signature::EcdsaKeyPair::from_pkcs8(alg, doc.as_ref(), &rng)
            .map_err(|_| Error::Cert("keygen"))?;
        Ok(SigningKey::EcP256(Box::new(kp)))
    }

    /// RSA keys only (what the SP stores).
    pub fn rsa_from_pkcs8_pem(pem: &str) -> Result<SigningKey, Error> {
        Ok(SigningKey::Rsa(Box::new(
            rsa::RsaPrivateKey::from_pkcs8_pem(pem).map_err(|_| Error::Cert("key"))?,
        )))
    }

    pub fn rsa_pkcs8_pem(&self) -> Result<String, Error> {
        match self {
            SigningKey::Rsa(k) => Ok(k
                .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
                .map_err(|_| Error::Cert("key"))?
                .to_string()),
            SigningKey::EcP256(_) => Err(Error::Cert("key")),
        }
    }

    pub fn alg(&self) -> &'static str {
        match self {
            SigningKey::Rsa(_) => RSA_SHA256,
            SigningKey::EcP256(_) => ECDSA_SHA256,
        }
    }

    pub fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, Error> {
        match self {
            SigningKey::Rsa(k) => {
                use rsa::signature::{SignatureEncoding, Signer};
                let sk = rsa::pkcs1v15::SigningKey::<Sha256>::new((**k).clone());
                Ok(sk
                    .try_sign(msg)
                    .map_err(|_| Error::Signature("sign"))?
                    .to_vec())
            }
            SigningKey::EcP256(kp) => {
                let rng = ring::rand::SystemRandom::new();
                Ok(kp
                    .sign(&rng, msg)
                    .map_err(|_| Error::Signature("sign"))?
                    .as_ref()
                    .to_vec())
            }
        }
    }

    fn spki(&self) -> Result<SubjectPublicKeyInfoOwned, Error> {
        let der = match self {
            SigningKey::Rsa(k) => k
                .to_public_key()
                .to_public_key_der()
                .map_err(|_| Error::Cert("spki"))?
                .as_bytes()
                .to_vec(),
            SigningKey::EcP256(kp) => {
                use ring::signature::KeyPair;
                let point = kp.public_key().as_ref();
                let alg = x509_cert::spki::AlgorithmIdentifierOwned {
                    oid: der::asn1::ObjectIdentifier::new_unwrap("1.2.840.10045.2.1"),
                    parameters: Some(der::Any::from(der::asn1::ObjectIdentifier::new_unwrap(
                        "1.2.840.10045.3.1.7",
                    ))),
                };
                let spki = SubjectPublicKeyInfoOwned {
                    algorithm: alg,
                    subject_public_key: der::asn1::BitString::from_bytes(point)
                        .map_err(|_| Error::Cert("spki"))?,
                };
                spki.to_der().map_err(|_| Error::Cert("spki"))?
            }
        };
        SubjectPublicKeyInfoOwned::from_der(&der).map_err(|_| Error::Cert("spki"))
    }

    /// Certificate (PEM) for this key, valid `days`. RSA keys self-sign;
    /// EC keys get a throwaway RSA issuer (only the public key matters: IdP
    /// certificates are pinned, never chain-validated).
    pub fn certificate(&self, common_name: &str, days: u32) -> Result<String, Error> {
        let cn: String = common_name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || ".-_ ".contains(*c))
            .take(60)
            .collect();
        let subject = Name::from_str(&format!("CN={}", if cn.is_empty() { "pelty" } else { &cn }))
            .map_err(|_| Error::Cert("name"))?;
        let issuer_key;
        let signer_rsa = match self {
            SigningKey::Rsa(k) => (**k).clone(),
            SigningKey::EcP256(_) => {
                issuer_key = rsa::RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048)
                    .map_err(|_| Error::Cert("keygen"))?;
                issuer_key
            }
        };
        let signer = rsa::pkcs1v15::SigningKey::<Sha256>::new(signer_rsa);
        let validity = Validity::from_now(std::time::Duration::from_secs(u64::from(days) * 86_400))
            .map_err(|_| Error::Cert("validity"))?;
        let mut serial = [0u8; 16];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut serial);
        serial[0] &= 0x7f;
        let serial = SerialNumber::new(&serial).map_err(|_| Error::Cert("serial"))?;
        let profile = match self {
            SigningKey::Rsa(_) => Profile::Root,
            SigningKey::EcP256(_) => Profile::Leaf {
                issuer: Name::from_str("CN=pelty-test-issuer").map_err(|_| Error::Cert("name"))?,
                enable_key_agreement: false,
                enable_key_encipherment: false,
            },
        };
        let b = CertificateBuilder::new(profile, serial, validity, subject, self.spki()?, &signer)
            .map_err(|_| Error::Cert("build"))?;
        let cert = b
            .build::<rsa::pkcs1v15::Signature>()
            .map_err(|_| Error::Cert("build"))?;
        Ok(cert_pem(&cert.to_der().map_err(|_| Error::Cert("build"))?))
    }
}
