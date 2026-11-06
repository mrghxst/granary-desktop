//! TLS failures are reported as specific, user-facing errors (Phase 4 "TLS errors shown clearly", ADR-0004 L3):
//! a local TLS server with a self-signed certificate must yield `tls_untrusted`, not a generic network error.
#![allow(clippy::unwrap_used, clippy::expect_used)] // test code

use granary_silo_api::device::DeviceIdentity;
use granary_silo_api::{ApiError, HttpSiloApi, SiloApi, TlsProblem};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

async fn self_signed_server() -> u16 {
    let ck = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    let cert: CertificateDer<'static> = ck.cert.der().clone();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(ck.signing_key.serialize_der()));
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let _ = acceptor.accept(tcp).await; // the client aborts the handshake
            });
        }
    });
    port
}

#[tokio::test]
async fn self_signed_certificate_is_reported_as_untrusted() {
    let port = self_signed_server().await;
    let api = HttpSiloApi::new(DeviceIdentity::fixed("8a1e5c2b-0d3f-4e7a-9b6c-1f2e3d4c5b6a")).unwrap();
    let err = api.system_info(&format!("https://localhost:{port}")).await.unwrap_err();
    assert!(
        matches!(
            err,
            ApiError::Tls {
                kind: TlsProblem::UntrustedIssuer,
                ..
            }
        ),
        "expected an untrusted-issuer TLS error, got {err:?}"
    );
    assert_eq!(err.code(), "tls_untrusted");
}
