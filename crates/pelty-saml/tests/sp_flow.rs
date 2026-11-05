//! End-to-end SP validation against a simulated IdP with locally generated
//! keys, including the classic attacks.
use pelty_saml::dsig::PublicKey;
use pelty_saml::keys::SigningKey;
use pelty_saml::sp::*;
use pelty_saml::testing::{sign_enveloped, ResponseSpec, TestIdp};
use pelty_saml::Error;
use std::sync::OnceLock;
use time::{Duration, OffsetDateTime};

const SP_ID: &str = "https://billing.example/sp";
const ACS: &str = "https://billing.example/api/v1/sso/saml/x/acs";
const IDP_ID: &str = "https://idp.example/entity";

fn rsa_idp() -> &'static TestIdp {
    static I: OnceLock<TestIdp> = OnceLock::new();
    I.get_or_init(|| TestIdp::new(IDP_ID, false).unwrap())
}
fn ec_idp() -> &'static TestIdp {
    static I: OnceLock<TestIdp> = OnceLock::new();
    I.get_or_init(|| TestIdp::new(IDP_ID, true).unwrap())
}

fn sp() -> Sp<'static> {
    Sp {
        entity_id: SP_ID,
        acs_url: ACS,
        slo_url: "https://billing.example/slo",
    }
}

fn check(idp: &TestIdp, xml: &str, req: Option<&str>) -> Result<Assertion, Error> {
    let keys = [PublicKey::from_cert(&idp.cert_pem).unwrap()];
    validate_response(
        xml,
        &sp(),
        &Idp {
            entity_id: IDP_ID,
            keys: &keys,
        },
        &Expect {
            request_id: req,
            allow_unsolicited: false,
            require_signed_assertion: true,
            now: OffsetDateTime::now_utc(),
            skew: Duration::seconds(60),
        },
    )
}

fn spec(req: &str) -> ResponseSpec {
    let mut s = ResponseSpec::new(IDP_ID, SP_ID, ACS, "ana@example.com", Some(req));
    s.attributes = vec![
        ("email".into(), vec!["ana@example.com".into()]),
        ("groups".into(), vec!["staff".into(), "vip".into()]),
    ];
    s
}

#[test]
fn valid_rsa_and_ec_responses() {
    for idp in [rsa_idp(), ec_idp()] {
        let xml = idp.response(&spec("_req1")).unwrap();
        let a = check(idp, &xml, Some("_req1")).unwrap();
        assert_eq!(a.name_id, "ana@example.com");
        assert_eq!(a.all("groups"), ["staff", "vip"]);
        assert_eq!(a.first("email"), Some("ana@example.com"));
        assert!(a.replay_until > OffsetDateTime::now_utc());
    }
    // Response + assertion both signed.
    let mut s = spec("_req1");
    s.sign_response = true;
    let xml = rsa_idp().response(&s).unwrap();
    assert!(check(rsa_idp(), &xml, Some("_req1")).is_ok());
}

#[test]
fn whitespace_and_prefix_variants_survive_c14n() {
    let idp = rsa_idp();
    let xml = idp.response(&spec("_r")).unwrap();
    // Pretty-printing *outside* the signed assertion doesn't matter.
    let pretty = xml.replace("<samlp:Status>", "\n  <samlp:Status>");
    assert!(check(idp, &pretty, Some("_r")).is_ok());
}

#[test]
fn rejects_tampering_and_wrong_key() {
    let idp = rsa_idp();
    let xml = idp.response(&spec("_r")).unwrap();
    let tampered = xml.replace(
        "ana@example.com</saml:NameID>",
        "admin@example.com</saml:NameID>",
    );
    assert_eq!(
        check(idp, &tampered, Some("_r")),
        Err(Error::Signature("digest_mismatch"))
    );
    // Signed by another IdP's key.
    let other = ec_idp().response(&spec("_r")).unwrap();
    assert!(check(idp, &other, Some("_r")).is_err());
    // Corrupt SignatureValue.
    let i = xml.find("<ds:SignatureValue>").unwrap() + 19;
    let mut b = xml.clone().into_bytes();
    b[i] = if b[i] == b'A' { b'B' } else { b'A' };
    assert!(check(idp, &String::from_utf8(b).unwrap(), Some("_r")).is_err());
}

#[test]
fn rejects_unsigned() {
    let mut s = spec("_r");
    s.sign_assertion = false;
    let xml = rsa_idp().response(&s).unwrap();
    assert_eq!(
        check(rsa_idp(), &xml, Some("_r")),
        Err(Error::Signature("missing"))
    );
    // Only the Response signed while assertions must be signed.
    s.sign_response = true;
    let xml = rsa_idp().response(&s).unwrap();
    assert_eq!(
        check(rsa_idp(), &xml, Some("_r")),
        Err(Error::Signature("missing"))
    );
}

#[test]
fn rejects_signature_wrapping() {
    let idp = rsa_idp();
    let good = spec("_r");
    let signed = sign_enveloped(&idp.assertion_xml(&good), &good.assertion_id, &idp.key).unwrap();
    // Attacker assertion with a fresh ID next to the genuine signed one.
    let mut evil = spec("_r");
    evil.name_id = "admin@example.com".into();
    let evil_a = idp
        .assertion_xml(&evil)
        .replace(&format!("<!--SIG:{}-->", evil.assertion_id), "");
    let two = idp.wrap(&good, &format!("{evil_a}{signed}")).unwrap();
    assert_eq!(
        check(idp, &two, Some("_r")),
        Err(Error::Invalid("assertion_count"))
    );
    // Evil assertion reusing the signed assertion's ID, with the original hidden in Extensions.
    let mut evil2 = spec("_r");
    evil2.assertion_id = good.assertion_id.clone();
    evil2.name_id = "admin@example.com".into();
    let evil2_a = idp
        .assertion_xml(&evil2)
        .replace(&format!("<!--SIG:{}-->", evil2.assertion_id), "");
    let hidden = idp
        .wrap(
            &good,
            &format!("<samlp:Extensions>{signed}</samlp:Extensions>{evil2_a}"),
        )
        .unwrap();
    assert!(check(idp, &hidden, Some("_r")).is_err());
    // Signature moved into the evil assertion, referencing the hidden original.
    let sig_start = signed.find("<ds:Signature").unwrap();
    let sig_end = signed.find("</ds:Signature>").unwrap() + 15;
    let sig = &signed[sig_start..sig_end];
    let original_unsigned = format!("{}{}", &signed[..sig_start], &signed[sig_end..]);
    let mut evil3 = spec("_r");
    evil3.name_id = "admin@example.com".into();
    let evil3_a = idp
        .assertion_xml(&evil3)
        .replace(&format!("<!--SIG:{}-->", evil3.assertion_id), sig);
    let moved = idp
        .wrap(
            &good,
            &format!("<samlp:Extensions>{original_unsigned}</samlp:Extensions>{evil3_a}"),
        )
        .unwrap();
    assert!(check(idp, &moved, Some("_r")).is_err());
    // Duplicate IDs anywhere are refused outright.
    let dup = signed.replace(
        "<saml:Issuer>",
        &format!(
            "<saml:Issuer ID=\"{}\" xmlns:x=\"urn:x\">",
            good.assertion_id
        ),
    );
    assert!(check(idp, &idp.wrap(&good, &dup).unwrap(), Some("_r")).is_err());
}

#[test]
fn rejects_expired_future_and_skew_is_configurable() {
    let idp = rsa_idp();
    let mut s = spec("_r");
    s.issued = OffsetDateTime::now_utc() - Duration::minutes(20);
    let xml = idp.response(&s).unwrap();
    assert_eq!(
        check(idp, &xml, Some("_r")).unwrap_err(),
        Error::Invalid("subject_confirmation")
    );
    let mut f = spec("_r");
    f.issued = OffsetDateTime::now_utc() + Duration::minutes(10);
    assert!(check(idp, &idp.response(&f).unwrap(), Some("_r")).is_err());
    // 30 s in the future is fine with the default 60 s skew, not with 0.
    let mut n = spec("_r");
    n.issued = OffsetDateTime::now_utc() + Duration::seconds(30);
    let xml = idp.response(&n).unwrap();
    assert!(check(idp, &xml, Some("_r")).is_ok());
    let keys = [PublicKey::from_cert(&idp.cert_pem).unwrap()];
    let strict = validate_response(
        &xml,
        &sp(),
        &Idp {
            entity_id: IDP_ID,
            keys: &keys,
        },
        &Expect {
            request_id: Some("_r"),
            allow_unsolicited: false,
            require_signed_assertion: true,
            now: OffsetDateTime::now_utc(),
            skew: Duration::ZERO,
        },
    );
    assert!(strict.is_err());
}

#[test]
fn rejects_wrong_audience_destination_issuer_and_request() {
    let idp = rsa_idp();
    let mut a = spec("_r");
    a.audience = "https://other.example/sp".into();
    assert_eq!(
        check(idp, &idp.response(&a).unwrap(), Some("_r")),
        Err(Error::Invalid("audience"))
    );
    let mut d = spec("_r");
    d.destination = "https://evil.example/acs".into();
    assert_eq!(
        check(idp, &idp.response(&d).unwrap(), Some("_r")),
        Err(Error::Invalid("destination"))
    );
    let mut r = spec("_r");
    r.recipient = "https://evil.example/acs".into();
    assert_eq!(
        check(idp, &idp.response(&r).unwrap(), Some("_r")),
        Err(Error::Invalid("subject_confirmation"))
    );
    let mut i = spec("_r");
    i.issuer = "https://evil-idp".into();
    assert_eq!(
        check(idp, &idp.response(&i).unwrap(), Some("_r")),
        Err(Error::Invalid("issuer"))
    );
    let xml = idp.response(&spec("_r")).unwrap();
    assert_eq!(
        check(idp, &xml, Some("_other")),
        Err(Error::Invalid("in_response_to"))
    );
    // Unsolicited is refused unless allowed.
    let u = ResponseSpec::new(IDP_ID, SP_ID, ACS, "ana@example.com", None);
    let xml = idp.response(&u).unwrap();
    assert_eq!(check(idp, &xml, None), Err(Error::Invalid("unsolicited")));
    let keys = [PublicKey::from_cert(&idp.cert_pem).unwrap()];
    let ok = validate_response(
        &xml,
        &sp(),
        &Idp {
            entity_id: IDP_ID,
            keys: &keys,
        },
        &Expect {
            request_id: None,
            allow_unsolicited: true,
            require_signed_assertion: true,
            now: OffsetDateTime::now_utc(),
            skew: Duration::seconds(60),
        },
    );
    assert!(ok.is_ok());
}

#[test]
fn rejects_doctype_and_encrypted() {
    let idp = rsa_idp();
    let xml = idp.response(&spec("_r")).unwrap();
    assert!(check(
        idp,
        &format!("<!DOCTYPE x [<!ENTITY a \"b\">]>{xml}"),
        Some("_r")
    )
    .is_err());
    let enc = idp.wrap(&spec("_r"), "<saml:EncryptedAssertion/>").unwrap();
    assert_eq!(
        check(idp, &enc, Some("_r")),
        Err(Error::Invalid("encrypted_assertion_unsupported"))
    );
}

#[test]
fn redirect_binding_roundtrip_and_signature() {
    let key = SigningKey::generate_ec().unwrap();
    let cert = key.certificate("sp", 1).unwrap();
    let pk = PublicKey::from_cert(&cert).unwrap();
    let xml = authn_request(
        "_abc",
        &sp(),
        "https://idp.example/sso",
        OffsetDateTime::now_utc(),
    );
    let url = redirect_url(
        "https://idp.example/sso",
        "SAMLRequest",
        &xml,
        Some("relay 1"),
        Some(&key),
    )
    .unwrap();
    let q = url.split_once('?').unwrap().1;
    let m = parse_redirect(q, std::slice::from_ref(&pk)).unwrap();
    assert!(m.signed);
    assert_eq!(m.xml, xml);
    assert_eq!(m.relay.as_deref(), Some("relay 1"));
    // Tampered relay invalidates the signature; unsigned is refused when keys are set.
    assert!(parse_redirect(
        &q.replace("relay%201", "relay%202"),
        std::slice::from_ref(&pk)
    )
    .is_err());
    let unsigned =
        redirect_url("https://idp.example/sso", "SAMLRequest", &xml, None, None).unwrap();
    assert!(parse_redirect(
        unsigned.split_once('?').unwrap().1,
        std::slice::from_ref(&pk)
    )
    .is_err());
    assert!(parse_redirect(unsigned.split_once('?').unwrap().1, &[]).is_ok());
}

#[test]
fn metadata_roundtrip() {
    let idp = rsa_idp();
    let md =
        parse_idp_metadata(&idp.metadata("https://idp.example/sso", "https://idp.example/slo"))
            .unwrap();
    assert_eq!(md.entity_id, IDP_ID);
    assert_eq!(md.sso_url, "https://idp.example/sso");
    assert_eq!(md.slo_url, "https://idp.example/slo");
    assert_eq!(md.certs.len(), 1);
    assert!(PublicKey::from_cert(&md.certs[0]).is_ok());
    let spm = sp_metadata(&sp(), Some(&idp.cert_pem), true);
    assert!(
        spm.contains("AssertionConsumerService")
            && spm.contains(ACS)
            && spm.contains("X509Certificate")
    );
}
