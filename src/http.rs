use std::sync::Once;

static INIT: Once = Once::new();

fn ensure_crypto_provider() {
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub fn new_client() -> reqwest::Client {
    ensure_crypto_provider();
    reqwest::Client::new()
}

pub fn client_builder() -> reqwest::ClientBuilder {
    ensure_crypto_provider();
    reqwest::Client::builder()
}
