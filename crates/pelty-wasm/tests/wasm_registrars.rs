//! WASM port of the native registrar behaviour tests
//! (`extensions/addons/reg-*/src`). RDAP and pure helpers stay
//! host-side and keep their native tests.
mod common;

mod registrars {
    use pelty_addon_registrar_opensrs::opensrs::opensrs_signature;
    use pelty_provisioning::registrar::*;
    use pelty_provisioning::registrars::*;
    use serde_json::{json, Value};
    use std::sync::Arc;

    /// Host HTTP for the addon, recorded/scripted by the native mock.
    struct Bridge(MockRegTransport);
    impl pelty_payments::HttpTransport for Bridge {
        fn send(
            &self,
            r: pelty_payments::HttpRequestSpec,
        ) -> pelty_payments::BoxFuture<'_, Result<(u16, String), String>> {
            self.0.send(RegRequest {
                method: r.method,
                url: r.url,
                headers: r.headers,
                body: r.body,
            })
        }
    }

    /// Same as the native helper, but the registrar is its compiled addon.
    fn reg(id: &str, settings: Value) -> (Arc<dyn RegistrarModule>, MockRegTransport) {
        let t = MockRegTransport::default();
        let zip =
            crate::common::addon_zip(&format!("reg-{id}"), &format!("pelty-addon-registrar-{id}"));
        let services = pelty_wasm::HostServices {
            http: Arc::new(Bridge(t.clone())),
            ..Default::default()
        };
        let addon = pelty_wasm::WasmAddon::from_zip(
            &zip,
            pelty_extensions::addon_pkg::Limits::default(),
            services,
        )
        .expect("load addon");
        let base =
            pelty_wasm::provisioning::WasmRegistrar::new(Arc::new(addon)).expect("registrar");
        assert_eq!(base.id(), id);
        (base.configure(&settings).unwrap(), t)
    }
    fn contact() -> Contact {
        Contact {
            name: "Ada Lovelace".into(),
            email: "ada@example.com".into(),
            phone: "+40.700000000".into(),
            address: "Str 1".into(),
            city: "Cluj".into(),
            postal_code: "400000".into(),
            country: "RO".into(),
            ..Default::default()
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unconfigured_refuses_without_network() {
        let (r, t) = reg("namecheap", json!({}));
        assert!(
            matches!(r.available("a.com").await, Err(RegistrarError::Remote(m)) if m.contains("not configured"))
        );
        assert!(t.requests().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn namecheap_flow() {
        let (r, t) = reg(
            "namecheap",
            json!({"api_user": "u", "api_key": "k", "username": "u", "client_ip": "1.2.3.4", "test_mode": true}),
        );
        t.push(200, r#"<ApiResponse Status="OK"><CommandResponse><DomainCheckResult Domain="pelty.com" Available="true" /></CommandResponse></ApiResponse>"#);
        assert!(r.available("pelty.com").await.unwrap());
        let q = &t.requests()[0];
        assert!(q
            .url
            .starts_with("https://api.sandbox.namecheap.com/xml.response?"));
        assert_eq!(
            q.query("Command").as_deref(),
            Some("namecheap.domains.check")
        );
        assert_eq!(q.query("DomainList").as_deref(), Some("pelty.com"));
        t.push(200, r#"<ApiResponse Status="OK"><DomainCreateResult Domain="pelty.com" Registered="true" ChargedAmount="9.0"/></ApiResponse>"#);
        r.register(
            "pelty.com",
            2,
            "2028-01-01",
            &contact(),
            &["ns1.a.io".into(), "ns2.a.io".into()],
        )
        .await
        .unwrap();
        let q = &t.requests()[1];
        assert_eq!(q.query("Years").as_deref(), Some("2"));
        assert_eq!(q.query("Nameservers").as_deref(), Some("ns1.a.io,ns2.a.io"));
        assert_eq!(q.query("RegistrantFirstName").as_deref(), Some("Ada"));
        assert_eq!(
            q.query("AuxBillingEmailAddress").as_deref(),
            Some("ada@example.com")
        );
        t.push(200, r#"<ApiResponse Status="OK"><DomainGetInfoResult Status="Ok"><DomainDetails><ExpiredDate>03/07/2028</ExpiredDate></DomainDetails><DnsDetails><Nameserver>dns1.x</Nameserver><Nameserver>dns2.x</Nameserver></DnsDetails></DomainGetInfoResult></ApiResponse>"#);
        let i = r.info("pelty.com").await.unwrap();
        assert_eq!(
            i,
            RemoteDomain {
                expires_on: "2028-03-07".into(),
                status: "active".into(),
                nameservers: vec!["dns1.x".into(), "dns2.x".into()]
            }
        );
        t.push(200, r#"<ApiResponse Status="ERROR"><Errors><Error Number="2030280">Domain is not available</Error></Errors></ApiResponse>"#);
        assert!(matches!(
            r.register("taken.com", 1, "", &contact(), &[]).await,
            Err(RegistrarError::Unavailable)
        ));
        assert_eq!(r.epp_code("pelty.com").await.unwrap(), None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resellerclub_flow() {
        let (r, t) = reg(
            "resellerclub",
            json!({"reseller_id": "1", "api_key": "k", "customer_id": "7", "contact_id": "9"}),
        );
        t.push(
            200,
            r#"{"pelty.ro":{"classkey":"dotro","status":"available"}}"#,
        );
        assert!(r.available("pelty.ro").await.unwrap());
        assert_eq!(t.requests()[0].query("tlds").as_deref(), Some("ro"));
        assert!(t.requests()[0]
            .url
            .starts_with("https://httpapi.com/api/domains/available.json?"));
        t.push(
            200,
            r#"{"actiontypedesc":"Registration","actionstatus":"Success","entityid":"55"}"#,
        );
        r.register(
            "pelty.ro",
            1,
            "",
            &contact(),
            &["ns1.x".into(), "ns2.x".into()],
        )
        .await
        .unwrap();
        let b = &t.requests()[1].body;
        assert!(
            b.contains("ns=ns1.x&ns=ns2.x")
                && b.contains("reg-contact-id=9")
                && b.contains("customer-id=7")
        );
        // renew: order id lookup, details, renew
        t.push(200, "55")
            .push(200, r#"{"endtime":"1830297600"}"#)
            .push(200, r#"{"actionstatus":"Success"}"#);
        r.renew("pelty.ro", 1, "2028-01-01").await.unwrap();
        assert!(t.requests()[4].body.contains("exp-date=1830297600"));
        t.push(200, r#"{"endtime":"1830297600","currentstatus":"Active","ns1":"a.ns","ns2":"b.ns","domsecret":"S3cr3t"}"#);
        let i = r.info("pelty.ro").await.unwrap();
        assert_eq!(i.expires_on, "2028-01-01");
        assert_eq!(i.nameservers, vec!["a.ns".to_string(), "b.ns".into()]);
        t.push(200, r#"{"domsecret":"S3cr3t"}"#);
        assert_eq!(
            r.epp_code("pelty.ro").await.unwrap().as_deref(),
            Some("S3cr3t")
        );
        t.push(500, r#"{"status":"ERROR","message":"Invalid auth code"}"#);
        assert!(matches!(
            r.transfer("x.ro", "bad", "").await,
            Err(RegistrarError::BadAuthCode)
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn enom_flow() {
        let (r, t) = reg("enom", json!({"uid": "u", "password": "p"}));
        t.push(
            200,
            "<interface-response><RRPCode>210</RRPCode><ErrCount>0</ErrCount></interface-response>",
        );
        assert!(r.available("pelty.net").await.unwrap());
        let q = &t.requests()[0];
        assert_eq!(
            (
                q.query("sld").as_deref(),
                q.query("tld").as_deref(),
                q.query("command").as_deref()
            ),
            (Some("pelty"), Some("net"), Some("check"))
        );
        t.push(
            200,
            "<interface-response><RRPCode>211</RRPCode><ErrCount>0</ErrCount></interface-response>",
        );
        assert!(!r.available("google.net").await.unwrap());
        t.push(
            200,
            "<interface-response><RRPCode>200</RRPCode><ErrCount>0</ErrCount></interface-response>",
        );
        r.register("pelty.net", 1, "", &contact(), &["ns1.x".into()])
            .await
            .unwrap();
        assert_eq!(t.requests()[2].query("NS1").as_deref(), Some("ns1.x"));
        t.push(200, "<interface-response><GetDomainInfo><status><expiration>1/5/2029 9:00:00 AM</expiration><registrationstatus>Registered</registrationstatus></status><services><entry name=\"dnsserver\"><configuration><dns>ns1.e.com</dns><dns>ns2.e.com</dns></configuration></entry></services></GetDomainInfo><ErrCount>0</ErrCount></interface-response>");
        let i = r.info("pelty.net").await.unwrap();
        assert_eq!(i.expires_on, "2029-01-05");
        assert_eq!(i.nameservers.len(), 2);
        t.push(200, "<interface-response><ErrCount>1</ErrCount><errors><Err1>Domain name not found</Err1></errors></interface-response>");
        assert!(matches!(
            r.renew("zz.net", 1, "").await,
            Err(RegistrarError::UnknownDomain)
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn opensrs_signs_and_parses() {
        assert_eq!(
            opensrs_signature("", ""),
            "74be16979710d4c4e7c6647856088456"
        );
        let (r, t) = reg(
            "opensrs",
            json!({"username": "res", "api_key": "KEY", "test_mode": true}),
        );
        t.push(200, r#"<OPS_envelope><body><data_block><dt_assoc><item key="is_success">1</item><item key="response_code">210</item><item key="attributes"><dt_assoc><item key="status">available</item></dt_assoc></item></dt_assoc></data_block></body></OPS_envelope>"#);
        assert!(r.available("pelty.org").await.unwrap());
        let q = &t.requests()[0];
        assert_eq!(q.url, "https://horizon.opensrs.net:55443");
        assert_eq!(
            q.header("X-Signature").map(str::to_string),
            Some(opensrs_signature(&q.body, "KEY"))
        );
        assert!(
            q.body.contains("<item key=\"action\">LOOKUP</item>")
                && q.body.contains("<item key=\"domain\">pelty.org</item>")
        );
        t.push(200, r#"<dt_assoc><item key="is_success">1</item><item key="attributes"><dt_assoc><item key="registry_expiredate">2030-02-03 12:00:00</item><item key="nameserver_list"><dt_array><item key="0"><dt_assoc><item key="name">ns1.o.com</item></dt_assoc></item></dt_array></item></dt_assoc></item></dt_assoc>"#);
        let i = r.info("pelty.org").await.unwrap();
        assert_eq!(
            (i.expires_on.as_str(), i.nameservers.len()),
            ("2030-02-03", 1)
        );
        t.push(200, r#"<item key="is_success">1</item>"#);
        r.renew("pelty.org", 2, "2032-02-03").await.unwrap();
        assert!(t.requests()[2]
            .body
            .contains("<item key=\"currentexpirationyear\">2030</item>"));
        t.push(200, r#"<item key="is_success">0</item><item key="response_text">Invalid domain authorization code</item>"#);
        assert!(matches!(
            r.transfer("x.org", "no", "").await,
            Err(RegistrarError::BadAuthCode)
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn godaddy_flow() {
        let (r, t) = reg("godaddy", json!({"api_key": "K", "api_secret": "S"}));
        t.push(200, r#"{"available":true,"domain":"pelty.io"}"#);
        assert!(r.available("pelty.io").await.unwrap());
        let q = &t.requests()[0];
        assert_eq!(q.header("Authorization"), Some("sso-key K:S"));
        assert_eq!(
            q.url,
            "https://api.godaddy.com/v1/domains/available?domain=pelty.io"
        );
        t.push(200, r#"{"orderId":1}"#);
        r.register("pelty.io", 1, "", &contact(), &["ns1.g".into()])
            .await
            .unwrap();
        let b: Value = serde_json::from_str(&t.requests()[1].body).unwrap();
        assert_eq!(b["contactRegistrant"]["nameLast"], "Lovelace");
        assert_eq!(b["consent"]["agreementKeys"][0], "DNRA");
        t.push(200, r#"{"expires":"2027-06-01T00:00:00.000Z","status":"ACTIVE","nameServers":["a","b"],"authCode":"GD-1"}"#);
        assert_eq!(r.info("pelty.io").await.unwrap().expires_on, "2027-06-01");
        t.push(200, r#"{"authCode":"GD-1"}"#);
        assert_eq!(
            r.epp_code("pelty.io").await.unwrap().as_deref(),
            Some("GD-1")
        );
        t.push(404, r#"{"code":"NOT_FOUND","message":"x"}"#);
        assert!(matches!(
            r.info("nope.io").await,
            Err(RegistrarError::UnknownDomain)
        ));
        t.push(204, "");
        r.set_nameservers("pelty.io", &["n1".into()]).await.unwrap();
        assert_eq!(t.requests()[5].method, "PATCH");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn centralnic_flow() {
        let (r, t) = reg(
            "centralnic",
            json!({"login": "l", "password": "p", "contact_handle": "P-ABC1"}),
        );
        t.push(
            200,
            "[RESPONSE]\ncode = 210\ndescription = Domain name available\nEOF\n",
        );
        assert!(r.available("pelty.xyz").await.unwrap());
        t.push(
            200,
            "[RESPONSE]\ncode = 211\ndescription = Domain name not available\nEOF\n",
        );
        assert!(!r.available("google.xyz").await.unwrap());
        t.push(
            200,
            "[RESPONSE]\ncode = 200\ndescription = Command completed successfully\nEOF\n",
        );
        r.register(
            "pelty.xyz",
            1,
            "",
            &contact(),
            &["ns1.c".into(), "ns2.c".into()],
        )
        .await
        .unwrap();
        let q = &t.requests()[2];
        assert_eq!(
            (
                q.query("command").as_deref(),
                q.query("ownercontact0").as_deref(),
                q.query("nameserver1").as_deref()
            ),
            (Some("AddDomain"), Some("P-ABC1"), Some("ns2.c"))
        );
        t.push(200, "[RESPONSE]\ncode = 200\ndescription = OK\nproperty[registrationexpirationdate][0] = 2028-09-09 10:00:00\nproperty[nameserver][0] = ns1.c\nproperty[status][0] = ACTIVE\nproperty[auth][0] = Xy-9\nEOF\n");
        let i = r.info("pelty.xyz").await.unwrap();
        assert_eq!(
            (
                i.expires_on.as_str(),
                i.nameservers.len(),
                i.status.as_str()
            ),
            ("2028-09-09", 1, "active")
        );
        t.push(
            200,
            "[RESPONSE]\ncode = 200\nproperty[auth][0] = Xy-9\nEOF\n",
        );
        assert_eq!(
            r.epp_code("pelty.xyz").await.unwrap().as_deref(),
            Some("Xy-9")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn generic_signs_body() {
        let (r, t) = reg(
            "generic",
            json!({"api_base": "https://reg.example/api/", "auth_header": "X-Key", "auth_value": "abc", "hmac_secret": "s"}),
        );
        t.push(200, r#"{"available":false}"#);
        assert!(!r.available("a.dev").await.unwrap());
        assert_eq!(
            t.requests()[0].url,
            "https://reg.example/api/domains/a.dev/availability"
        );
        assert_eq!(t.requests()[0].header("X-Key"), Some("abc"));
        t.push(201, r#"{"ok":true}"#);
        r.register("a.dev", 1, "", &contact(), &[]).await.unwrap();
        let q = &t.requests()[1];
        use hmac::{digest::KeyInit, Hmac, Mac};
        let mut m = Hmac::<sha2::Sha256>::new_from_slice(b"s").unwrap();
        m.update(q.body.as_bytes());
        assert_eq!(
            q.header("X-Pelty-Signature").map(str::to_string),
            Some(hex::encode(m.finalize().into_bytes()))
        );
        t.push(
            200,
            r#"{"expires_on":"2027-01-02","status":"active","nameservers":["x"]}"#,
        );
        assert_eq!(r.info("a.dev").await.unwrap().expires_on, "2027-01-02");
    }

    /// Custom domain fields reach each registrar's wire format; credentials can't be overridden.
    #[tokio::test(flavor = "multi_thread")]
    async fn custom_fields_reach_registrars() {
        let f =
            json!({"RegistrantNexus": "C11", "ApiKey": "evil", "us_purpose": "P1", "pw": "evil"});
        let (r, t) = reg(
            "namecheap",
            json!({"api_user": "u", "api_key": "k", "username": "u", "client_ip": "1.2.3.4"}),
        );
        t.push(
            200,
            r#"<ApiResponse Status="OK"><DomainCreateResult Registered="true"/></ApiResponse>"#,
        );
        r.register_with_fields("a.us", 1, "", &contact(), &[], &f)
            .await
            .unwrap();
        let q = &t.requests()[0];
        assert_eq!(q.query("RegistrantNexus").as_deref(), Some("C11"));
        assert_eq!(q.query("ApiKey").as_deref(), Some("k"));
        assert_eq!(q.url.matches("ApiKey=").count(), 1);

        let (r, t) = reg("enom", json!({"uid": "u", "password": "p"}));
        t.push(
            200,
            "<interface-response><RRPCode>200</RRPCode><ErrCount>0</ErrCount></interface-response>",
        );
        r.register_with_fields("a.us", 1, "", &contact(), &[], &f)
            .await
            .unwrap();
        let q = &t.requests()[0];
        assert_eq!(q.query("us_purpose").as_deref(), Some("P1"));
        assert_eq!(q.query("pw").as_deref(), Some("p"));
        assert_eq!(q.url.matches("pw=").count(), 1);

        let (r, t) = reg(
            "opensrs",
            json!({"username": "res", "api_key": "KEY", "test_mode": true}),
        );
        t.push(200, r#"<item key="is_success">1</item>"#);
        r.register_with_fields("a.us", 1, "", &contact(), &[], &f)
            .await
            .unwrap();
        assert!(
            t.requests()[0].body.contains("tld_data")
                && t.requests()[0]
                    .body
                    .contains("<item key=\"RegistrantNexus\">C11</item>")
        );

        let (r, t) = reg("generic", json!({"api_base": "https://reg.example/api"}));
        t.push(201, r#"{"ok":true}"#);
        r.register_with_fields("a.us", 1, "", &contact(), &[], &f)
            .await
            .unwrap();
        let b: Value = serde_json::from_str(&t.requests()[0].body).unwrap();
        assert_eq!(b["fields"]["RegistrantNexus"], "C11");
        t.push(200, r#"{"ok":true}"#);
        r.transfer_with_fields("a.us", "EPP", "", &f).await.unwrap();
        let b: Value = serde_json::from_str(&t.requests()[1].body).unwrap();
        assert_eq!(
            (b["auth_code"].as_str(), b["fields"]["us_purpose"].as_str()),
            (Some("EPP"), Some("P1"))
        );
        // No fields: plain register still works and sends an empty object.
        t.push(201, r#"{"ok":true}"#);
        r.register("b.us", 1, "", &contact(), &[]).await.unwrap();
        let b: Value = serde_json::from_str(&t.requests()[2].body).unwrap();
        assert_eq!(b["fields"], json!({}));
    }

    fn form(body: &str, k: &str) -> Vec<String> {
        url::form_urlencoded::parse(body.as_bytes())
            .filter(|(a, _)| a == k)
            .map(|(_, v)| v.into_owned())
            .collect()
    }

    /// GoDaddy: JSON members (case kept, dotted keys nest, types kept); core members untouchable.
    #[tokio::test(flavor = "multi_thread")]
    async fn godaddy_custom_fields() {
        let (r, t) = reg("godaddy", json!({"api_key": "K", "api_secret": "S"}));
        let f = json!({"contactRegistrant.nexusCategory": "C11", "contactRegistrant.nexusPurpose": "P1", "renewAuto": true, "consent.agreementKeys": "x", "period": 9, "contactAdmin": "evil", "ExtraFlag": true, "Score": 3});
        t.push(200, r#"{"orderId":1}"#);
        r.register_with_fields("a.us", 2, "", &contact(), &["ns1.g".into()], &f)
            .await
            .unwrap();
        let b: Value = serde_json::from_str(&t.requests()[0].body).unwrap();
        assert_eq!(
            (
                b["contactRegistrant"]["nexusCategory"].as_str(),
                b["contactRegistrant"]["nexusPurpose"].as_str()
            ),
            (Some("C11"), Some("P1"))
        );
        assert_eq!(
            b["contactRegistrant"]["nameLast"], "Lovelace",
            "contact kept"
        );
        assert!(b["contactAdmin"]["nexusCategory"].is_null() && b["contactAdmin"].is_object());
        assert_eq!(
            (b["renewAuto"].as_bool(), b["period"].as_u64()),
            (Some(false), Some(2))
        );
        assert_eq!(b["consent"]["agreementKeys"][0], "DNRA");
        assert_eq!(
            (b["ExtraFlag"].as_bool(), b["Score"].as_u64()),
            (Some(true), Some(3))
        );
        t.push(200, r#"{"orderId":2}"#);
        r.transfer_with_fields(
            "a.us",
            "EPP-1",
            "",
            &json!({"contactRegistrant.nexusCategory": "C12", "authCode": "evil"}),
        )
        .await
        .unwrap();
        let b: Value = serde_json::from_str(&t.requests()[1].body).unwrap();
        assert_eq!(
            (
                b["authCode"].as_str(),
                b["contactRegistrant"]["nexusCategory"].as_str()
            ),
            (Some("EPP-1"), Some("C12"))
        );
        // No fields: body unchanged.
        t.push(200, r#"{"orderId":3}"#);
        r.register("b.io", 1, "", &contact(), &[]).await.unwrap();
        let b: Value = serde_json::from_str(&t.requests()[2].body).unwrap();
        assert!(
            b.get("ExtraFlag").is_none() && b["contactRegistrant"].get("nexusCategory").is_none()
        );
    }

    /// CentralNic / RRPproxy: X- extension parameters; credentials/commands untouchable.
    #[tokio::test(flavor = "multi_thread")]
    async fn centralnic_custom_fields() {
        let (r, t) = reg(
            "centralnic",
            json!({"login": "l", "password": "p", "contact_handle": "P-ABC1"}),
        );
        let f = json!({"X-CA-LEGALTYPE": "CCT", "us_nexus_category": "C11", "X-ACCEPT-TRUSTEE-TAC": true, "s_pw": "evil", "command": "DeleteDomain"});
        let ok = "[RESPONSE]\ncode = 200\ndescription = OK\nEOF\n";
        t.push(200, ok);
        r.register_with_fields("a.ca", 1, "", &contact(), &["ns1.c".into()], &f)
            .await
            .unwrap();
        let q = &t.requests()[0];
        assert_eq!(q.query("X-CA-LEGALTYPE").as_deref(), Some("CCT"));
        assert_eq!(q.query("X-US-NEXUS-CATEGORY").as_deref(), Some("C11"));
        assert_eq!(q.query("X-ACCEPT-TRUSTEE-TAC").as_deref(), Some("1"));
        assert_eq!(
            (q.query("command").as_deref(), q.query("s_pw").as_deref()),
            (Some("AddDomain"), Some("p"))
        );
        assert_eq!(
            (
                q.url.matches("s_pw=").count(),
                q.url.matches("command=").count()
            ),
            (1, 1)
        );
        assert_eq!(
            q.query("X-S-PW").as_deref(),
            Some("evil"),
            "prefixed, never the credential itself"
        );
        t.push(200, ok);
        r.transfer_with_fields("a.ca", "AUTH", "", &json!({"X-CA-LEGALTYPE": "CCO"}))
            .await
            .unwrap();
        let q = &t.requests()[1];
        assert_eq!(
            (
                q.query("command").as_deref(),
                q.query("auth").as_deref(),
                q.query("X-CA-LEGALTYPE").as_deref()
            ),
            (Some("TransferDomain"), Some("AUTH"), Some("CCO"))
        );
    }

    /// ResellerClub / LogicBoxes: attr-nameN / attr-valueN, key case kept.
    #[tokio::test(flavor = "multi_thread")]
    async fn resellerclub_custom_fields() {
        let (r, t) = reg(
            "resellerclub",
            json!({"reseller_id": "1", "api_key": "k", "customer_id": "7", "contact_id": "9"}),
        );
        let f =
            json!({"purpose": "P1", "category": "C11", "CedContactId": "123", "api-key": "evil"});
        t.push(200, r#"{"actionstatus":"Success","entityid":"55"}"#);
        r.register_with_fields("a.us", 1, "", &contact(), &["ns1.x".into()], &f)
            .await
            .unwrap();
        let b = &t.requests()[0].body;
        let pairs: Vec<(String, String)> = (1..=3)
            .map(|i| {
                (
                    form(b, &format!("attr-name{i}"))[0].clone(),
                    form(b, &format!("attr-value{i}"))[0].clone(),
                )
            })
            .collect();
        assert!(
            pairs.contains(&("purpose".into(), "P1".into()))
                && pairs.contains(&("category".into(), "C11".into()))
                && pairs.contains(&("CedContactId".into(), "123".into())),
            "{pairs:?}"
        );
        assert!(form(b, "attr-name4").is_empty());
        assert_eq!(form(b, "api-key"), vec!["k".to_string()]);
        t.push(200, r#"{"actionstatus":"Success"}"#);
        r.transfer_with_fields("a.us", "EPP", "", &json!({"purpose": "P3"}))
            .await
            .unwrap();
        let b = &t.requests()[1].body;
        assert_eq!(
            (
                form(b, "auth-code"),
                form(b, "attr-name1"),
                form(b, "attr-value1")
            ),
            (
                vec!["EPP".into()],
                vec!["purpose".into()],
                vec!["P3".into()]
            )
        );
        t.push(200, r#"{"actionstatus":"Success"}"#);
        r.register("b.us", 1, "", &contact(), &[]).await.unwrap();
        assert!(!t.requests()[2].body.contains("attr-name"));
    }
}
