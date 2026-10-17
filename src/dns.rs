//! Provider hostname resolution (FR-022a–c, R-5, Constitution VII).
//!
//! The only network-capable code in Fallow. It resolves the configured provider hostnames,
//! forward only, one at a time, through the system resolver (`ToSocketAddrs`). It never does
//! reverse lookups and never resolves any other name.

use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use compact_str::CompactString;

use crate::config::Provider;
use crate::procmon::detect::ProviderSet;

/// The current provider set, swapped as a whole after each DNS pass.
pub type SharedProviders = Arc<Mutex<Arc<ProviderSet>>>;

/// A set with the built-in ranges only, used until (and whenever) lookups fail.
pub fn initial(providers: &[Provider]) -> SharedProviders {
    Arc::new(Mutex::new(Arc::new(ProviderSet::new(providers, []))))
}

/// One pass over every enabled provider hostname.
pub fn resolve(providers: &[Provider]) -> ProviderSet {
    let mut found = Vec::new();
    for p in providers.iter().filter(|p| p.enabled) {
        for host in &p.hostnames {
            if let Ok(addrs) = (host.as_str(), 443).to_socket_addrs() {
                found.extend(addrs.map(|a| (a.ip(), CompactString::from(p.id.as_str()))));
            }
        }
    }
    ProviderSet::new(providers, found)
}

/// Starts the `dns` thread: a pass now, then every `refresh_minutes`. Call it only after the
/// first frame (FR-022c), and not at all when DNS is disabled.
pub fn spawn(
    providers: Vec<Provider>,
    refresh_minutes: u64,
    shared: SharedProviders,
) -> std::io::Result<()> {
    thread::Builder::new()
        .name("dns".into())
        .stack_size(128 * 1024)
        .spawn(move || {
            loop {
                let set = resolve(&providers);
                if let Ok(mut s) = shared.lock() {
                    *s = Arc::new(set);
                }
                thread::sleep(Duration::from_secs(refresh_minutes.max(1) * 60));
            }
        })?;
    Ok(())
}
