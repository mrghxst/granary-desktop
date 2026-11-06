//! Live, read-only integration test against the server in `.env.local` (`SILO_URL`, `SILO_USERNAME`,
//! `SILO_PASSWORD`). Tagged `#[ignore]`: CI never runs it. Locally: `cargo test -p granary-silo-api --test live --
//! --ignored`. It signs in (creating one login session), reads account, profiles and libraries, and signs out again,
//! which ends that session. Credentials are never printed.
#![allow(clippy::unwrap_used, clippy::expect_used)] // test code

use granary_silo_api::credentials::MemoryStore;
use granary_silo_api::device::DeviceIdentity;
use granary_silo_api::{HttpSiloApi, Secret, Session, SiloApi, probe};
use std::collections::HashMap;
use std::sync::Arc;

fn env_local() -> Option<HashMap<String, String>> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.env.local");
    let text = std::fs::read_to_string(path).ok()?;
    Some(
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_owned(), v.trim().trim_matches('"').to_owned()))
            .collect(),
    )
}

#[tokio::test]
#[ignore = "live server; needs .env.local"]
async fn live_login_and_library_listing() {
    let Some(env) = env_local() else {
        eprintln!("SKIPPED: no .env.local");
        return;
    };
    let (url, user, pass) = (
        &env["SILO_URL"],
        &env["SILO_USERNAME"],
        Secret::new(env["SILO_PASSWORD"].clone()),
    );
    let api: Arc<dyn SiloApi> =
        Arc::new(HttpSiloApi::new(DeviceIdentity::fixed("00000000-0000-4000-8000-00000000d351")).unwrap());
    let p = probe(api.as_ref(), url).await.unwrap();
    eprintln!(
        "server {} api_major={} untested={}",
        p.info.server_version, p.info.api_major, p.untested
    );
    let store = Arc::new(MemoryStore::default());
    let (session, account) = Session::login(api, store, &p.origin, &p.server_id, user, &pass)
        .await
        .unwrap();
    eprintln!("signed in as user id {}", account.id);
    let profiles = session.profiles().await.unwrap();
    eprintln!("profiles: {}", profiles.len());
    if let Some(open) = profiles.iter().find(|p| !p.has_pin) {
        session.select_profile(&open.id, None).await.unwrap();
    }
    let libs = session.libraries().await.unwrap();
    eprintln!(
        "libraries: {:?}",
        libs.iter().map(|l| (&l.name, &l.kind)).collect::<Vec<_>>()
    );
    session.logout().await;
}
