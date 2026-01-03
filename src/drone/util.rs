use std::net::{IpAddr, SocketAddr};

use tokio::net::lookup_host;

use crate::GLOBAL_CONFIG;

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => ipv4.is_private() || ipv4.is_loopback() || ipv4.is_link_local(),
        IpAddr::V6(ipv6) => {
            // Loopback, unspecified, unique local (fc00::/7)
            if ipv6.is_loopback() || ipv6.is_unspecified() {
                return true;
            }

            // Check specifically for fdaa::/16
            let segments = ipv6.segments(); // 8 u16 segments
            if segments[0] == 0xfdaa {
                return true;
            }

            // Also block the general unique local addresses (fc00::/7)
            ipv6.is_unique_local()
        }
    }
}

pub async fn resolve_public_ip(url: &str) -> Option<SocketAddr> {
    let url = url::Url::parse(url).ok()?;

    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }

    let host = url.host_str()?;
    let port = url.port_or_known_default().unwrap_or(80);

    let addrs = lookup_host((host, port)).await.ok()?;

    let mut public_addr = None;
    let allow_private_addrs = GLOBAL_CONFIG.get().unwrap().is_dev;

    for addr in addrs {
        if allow_private_addrs {
            public_addr = Some(addr);
            break;
        }

        if !is_private_ip(&addr.ip()) {
            public_addr = Some(addr);
            break;
        }
    }

    public_addr
}
