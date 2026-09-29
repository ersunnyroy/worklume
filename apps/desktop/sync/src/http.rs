//! HTTPS for the agent. Every connection trusts the operating system's certificate store
//! (Keychain, the Windows store, `/etc/ssl` on Linux) rather than a bundled list, so the agent
//! keeps working behind corporate proxies that inspect TLS with their own root certificate.
//! Proxies come from `HTTPS_PROXY` and `ALL_PROXY`.

use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::{Agent, Proxy};

/// No single request may take longer than this, so a stalled connection cannot hold a worker.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// An HTTP client configured as above.
#[must_use]
pub fn agent() -> Agent {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::NativeTls)
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    Agent::config_builder()
        .tls_config(tls)
        .proxy(Proxy::try_from_env())
        .timeout_global(Some(REQUEST_TIMEOUT))
        .user_agent(concat!("worklume-desktop/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

#[cfg(test)]
mod tests {
    use ureq::tls::{RootCerts, TlsProvider};

    use super::{REQUEST_TIMEOUT, agent};

    #[test]
    fn trusts_the_operating_system_certificate_store() {
        let agent = agent();
        let tls = agent.config().tls_config();
        assert_eq!(tls.provider(), TlsProvider::NativeTls);
        assert!(matches!(tls.root_certs(), RootCerts::PlatformVerifier));
        assert_eq!(agent.config().timeouts().global, Some(REQUEST_TIMEOUT));
    }
}
