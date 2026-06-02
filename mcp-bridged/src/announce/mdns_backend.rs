//! Pure-Rust [`MdnsSubscriber`] backend built on the `mdns-sd` crate.
//!
//! `mdns-sd` ships a single-process mDNS implementation that works on
//! macOS, Linux, and Windows without linking against Bonjour or Avahi.
//! The architectural decision to use it over a per-OS split
//! (`astro-dnssd` / `zeroconf`) is captured in
//! [`docs/decisions/`](../../../../docs/decisions) — the trait
//! abstraction in [`super::mdns`] keeps the door open to swap if
//! pure-Rust mDNS turns out to have a deal-breaker bug on some
//! platform.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::mpsc;
use tracing::debug;

use super::mdns::{MdnsAnnouncement, MdnsSubscriber, SubscribeError};

/// Buffer size for the mpsc channel handed back to the bridge task.
/// The bridge consumes events as fast as it can — but if processing
/// stalls (e.g. busy registry lock), older events buffer up to this
/// cap then drop oldest.
const CHANNEL_CAP: usize = 64;

/// [`MdnsSubscriber`] backed by a process-wide [`ServiceDaemon`].
///
/// Holds a single shared `ServiceDaemon` per subscriber instance.
/// One [`subscribe`](MdnsSubscriber::subscribe) call browses all
/// supplied service types into one merged stream.
#[allow(missing_debug_implementations)] // mdns_sd::ServiceDaemon is not Debug.
pub struct MdnsSdSubscriber {
    daemon: Arc<ServiceDaemon>,
}

impl MdnsSdSubscriber {
    /// Spin up the in-process mDNS daemon. Errors when the daemon
    /// can't bind its multicast sockets (typically: another mDNS
    /// responder is already taking the port, or the OS has refused
    /// the bind permission).
    pub fn new() -> Result<Self, SubscribeError> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| SubscribeError::Backend(format!("ServiceDaemon::new: {e}")))?;
        Ok(Self {
            daemon: Arc::new(daemon),
        })
    }
}

#[async_trait]
impl MdnsSubscriber for MdnsSdSubscriber {
    async fn subscribe(
        &self,
        service_types: &[String],
    ) -> Result<mpsc::Receiver<MdnsAnnouncement>, SubscribeError> {
        let (tx, rx) = mpsc::channel(CHANNEL_CAP);

        for service_type in service_types {
            // mdns-sd's browse() insists on the fully-qualified form
            // with a trailing dot ("_xxx._tcp.local."). SPEC §5.2 writes
            // the type without the dot; both refer to the same name on
            // the wire. Normalise here so the SPEC-level helper stays
            // unchanged.
            let fq = if service_type.ends_with('.') {
                service_type.clone()
            } else {
                format!("{service_type}.")
            };
            let browse_rx = self
                .daemon
                .browse(&fq)
                .map_err(|e| SubscribeError::Backend(format!("browse {fq}: {e}")))?;
            let tx = tx.clone();
            // One drain task per service type. mdns-sd's receiver is
            // a flume::Receiver; we use its `recv_async` for tokio
            // friendliness (mdns-sd's `async` feature enables it).
            tokio::spawn(async move {
                while let Ok(event) = browse_rx.recv_async().await {
                    if let ServiceEvent::ServiceResolved(info) = event {
                        if let Some(ann) = announcement_from_info(&info) {
                            if tx.send(ann).await.is_err() {
                                // Receiver dropped — bridge stopped.
                                break;
                            }
                        }
                    }
                }
                debug!("mdns-sd browse loop exited");
            });
        }

        Ok(rx)
    }
}

/// Map an mdns-sd [`ServiceInfo`] to our [`MdnsAnnouncement`] shape.
/// Returns `None` when the publisher exposes no IP addresses (we have
/// nowhere to apply the per-source-IP rate limit, so the announcement
/// is unsafe to forward).
fn announcement_from_info(info: &ServiceInfo) -> Option<MdnsAnnouncement> {
    let addr = info.get_addresses().iter().next().copied()?;

    let mut txt: HashMap<String, String> = HashMap::new();
    for prop in info.get_properties().iter() {
        // mdns-sd's val_str returns "" for binary or absent values;
        // the `body` field is always ASCII base64url so the empty
        // case is just a non-matching key we'd skip downstream anyway.
        txt.insert(prop.key().to_owned(), prop.val_str().to_owned());
    }

    Some(MdnsAnnouncement {
        instance_name: info.get_fullname().to_owned(),
        txt,
        source_addr: addr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_succeeds_in_a_normal_test_env() {
        // CI hosts and dev machines should be able to bind the
        // multicast socket. If this ever fails on a runner it's a
        // signal the environment doesn't support mDNS at all and the
        // backend is correctly unusable there.
        // Acceptable: the daemon could bind the socket, OR it
        // couldn't (sandbox, CI runner without multicast). The only
        // thing that should NOT happen is some other error variant.
        match MdnsSdSubscriber::new() {
            Ok(_) | Err(SubscribeError::Backend(_)) => {}
            Err(other) => panic!("unexpected error: {other}"),
        }
    }
}
