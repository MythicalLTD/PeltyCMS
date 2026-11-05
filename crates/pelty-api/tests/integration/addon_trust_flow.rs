//! Trust policy applies to upload, direct disk installation, and re-enabling.
use crate::common::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};

use pelty_extensions::{addon_pkg, addon_signing};
use serde_json::json;
use std::{io::Write, sync::Arc};
use tower::ServiceExt;

fn zip(manifest: &[u8], sig: Option<&addon_signing::ReleaseSignature>) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("addon.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(manifest).unwrap();
    if let Some(sig) = sig {
        zip.start_file("addon.sig", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&serde_json::to_vec(sig).unwrap()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

async fn upload(app: &axum::Router, admin: &Sess, bytes: Vec<u8>) -> StatusCode {
    let mut req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/addons/upload")
        .header("content-type", "application/zip");
    for (k, v) in admin.h() {
        req = req.header(k, v);
    }
    app.clone()
        .oneshot(req.body(Body::from(bytes)).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn trust_is_enforced_before_extraction_install_and_enable() {
    let (app, mut state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let id = format!("trust-{}", uuid::Uuid::new_v4().simple());
    let manifest = serde_json::to_vec(&json!({"id":id,"name":"Trust fixture","version":"1.0.0","kind":"plugin","engine":"trust-test","files":{}})).unwrap();
    let unsigned = zip(&manifest, None);
    let pkg = addon_pkg::inspect(&unsigned, addon_pkg::Limits::default()).unwrap();
    let (private, public) = addon_signing::generate_keypair().unwrap();
    let sig = addon_signing::sign(&pkg, "fixture", &private).unwrap();
    let signed = zip(&manifest, Some(&sig));
    let path = std::env::temp_dir().join(format!("pelty-trust-{}.json", uuid::Uuid::new_v4()));
    let trust = |revoked| {
        serde_json::to_vec(&json!({"fixture":{"public_key":public,"revoked":revoked}})).unwrap()
    };
    std::fs::write(&path, trust(false)).unwrap();
    Arc::make_mut(&mut state.cfg).addon_trusted_keys = Some(path.clone());
    Arc::make_mut(&mut state.cfg).addon_require_signatures = true;
    let app = pelty_api::router(state.clone());
    let disk = pelty_api::addons::addons_dir().join(&id);
    assert_eq!(
        upload(&app, &admin, unsigned.clone()).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(!disk.exists());
    assert_eq!(upload(&app, &admin, signed.clone()).await, StatusCode::OK);
    let base = format!("/api/v1/admin/addons/{id}");
    assert_eq!(
        call(&app, "POST", &format!("{base}/install"), None, &admin.h())
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", &format!("{base}/enable"), None, &admin.h())
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "POST", &format!("{base}/disable"), None, &admin.h())
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    std::fs::write(&path, trust(true)).unwrap();
    assert_eq!(
        call(&app, "POST", &format!("{base}/enable"), None, &admin.h())
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(
        !pelty_database::addon_registry::get(&state.pool, &id)
            .await
            .unwrap()
            .unwrap()
            .enabled
    );
    std::fs::write(&path, b"{}").unwrap();
    assert!(pelty_api::addon_trust::verify_package(
        &state,
        &addon_pkg::inspect(&signed, addon_pkg::Limits::default()).unwrap()
    )
    .is_err());
    // Direct disk packages may not bypass trust enforcement.
    let direct_id = format!("trust-{}", uuid::Uuid::new_v4().simple());
    let direct = serde_json::to_vec(&json!({"id":direct_id,"name":"Unsigned disk","version":"1.0.0","kind":"plugin","engine":"trust-test","files":{}})).unwrap();
    let direct_pkg = addon_pkg::inspect(&zip(&direct, None), addon_pkg::Limits::default()).unwrap();
    let direct_dir =
        addon_pkg::extract(&direct_pkg, &pelty_api::addons::addons_dir(), false).unwrap();
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/addons/{direct_id}/install"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", r.body);
    // Malformed policy never falls back to unsigned acceptance.
    std::fs::write(&path, b"not json").unwrap();
    Arc::make_mut(&mut state.cfg).addon_require_signatures = false;
    assert!(pelty_api::addon_trust::verify_package(&state, &pkg).is_err());
    std::fs::remove_dir_all(disk).unwrap();
    std::fs::remove_dir_all(direct_dir).unwrap();
    std::fs::remove_file(path).unwrap();
}
