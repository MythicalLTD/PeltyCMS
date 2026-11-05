//! Responses signed by signxml (lxml/libxml2 C14N, an independent
//! implementation); see fixtures/gen_fixtures.py. Time is pinned to the
//! fixture's validity window.
use pelty_saml::dsig::PublicKey;
use pelty_saml::sp::*;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

fn run(kind: &str, xml: &str, at: &str) -> Result<Assertion, pelty_saml::Error> {
    let cert = std::fs::read_to_string(format!(
        "{}/tests/fixtures/{kind}.crt",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let keys = [PublicKey::from_cert(&cert).unwrap()];
    validate_response(
        xml,
        &Sp {
            entity_id: "https://billing.example/sp",
            acs_url: "https://billing.example/acs",
            slo_url: "",
        },
        &Idp {
            entity_id: "https://idp.fixture/entity",
            keys: &keys,
        },
        &Expect {
            request_id: Some("_req42"),
            allow_unsolicited: false,
            require_signed_assertion: true,
            now: OffsetDateTime::parse(at, &Rfc3339).unwrap(),
            skew: Duration::seconds(30),
        },
    )
}

fn fixture(kind: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/response_{kind}.xml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn verifies_signxml_rsa_and_ecdsa() {
    for kind in ["rsa", "ec"] {
        let a = run(kind, &fixture(kind), "2026-10-30T10:02:00Z")
            .unwrap_or_else(|e| panic!("{kind}: {e}"));
        assert_eq!(a.name_id, "ion@example.com");
        assert_eq!(a.all("memberOf"), ["Clients & Co", "vip"]);
        assert_eq!(a.first("email"), Some("ion@example.com"));
        assert_eq!(a.session_index.as_deref(), Some("s1"));
    }
}

#[test]
fn signxml_fixture_negatives() {
    let xml = fixture("rsa");
    assert!(run("rsa", &xml, "2026-10-30T10:06:00Z").is_err(), "expired");
    assert!(
        run("ec", &xml, "2026-10-30T10:02:00Z").is_err(),
        "wrong certificate"
    );
    assert!(
        run(
            "rsa",
            &xml.replace(">vip<", ">admin<"),
            "2026-10-30T10:02:00Z"
        )
        .is_err(),
        "tampered attribute"
    );
    assert!(xml.contains("\n    <saml:Conditions"));
    assert!(
        run(
            "rsa",
            &xml.replace("\n    <saml:Conditions", "\n  <saml:Conditions"),
            "2026-10-30T10:02:00Z"
        )
        .is_err(),
        "whitespace is signed"
    );
    // Whitespace outside the signed assertion is not.
    assert!(run(
        "rsa",
        &xml.replace("\n  <samlp:Status>", "\n\n  <samlp:Status>"),
        "2026-10-30T10:02:00Z"
    )
    .is_ok());
}
