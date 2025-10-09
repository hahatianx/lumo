//! Network utilities.
//! 
//! Provides a cross-platform helper to discover a private (RFC1918) IPv4 address
//! of the current machine. If multiple interfaces exist, any one private IPv4 may
//! be returned. If no private IPv4 address can be inferred, `None` is returned.
//! 
//! This implementation avoids external dependencies and should work on Linux,
//! macOS, and Windows.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};

/// Return a private (RFC1918) IPv4 address and its MAC address if both can be inferred.
///
/// This function prefers enumerating network interfaces (via `if_addrs`) to obtain a
/// private IPv4 along with its MAC. If that fails to yield a MAC, it falls back to the
/// UDP "connect" heuristic to infer the local private IPv4, then attempts to map that
/// IP back to an interface to fetch the MAC.
///
/// Returns `Some((ip, mac))` when both values are available; otherwise returns `None`.
pub fn get_private_ipv4_with_mac() -> Option<(Ipv4Addr, [u8; 6])> {
    // First, try to enumerate interfaces directly with pnet_datalink.
    {
        use pnet_datalink::NetworkInterface;
        let ifaces: Vec<NetworkInterface> = pnet_datalink::interfaces();
        for iface in &ifaces {
            if let Some(mac) = iface.mac {
                // Check IPv4 addresses on this interface
                for ipnet in &iface.ips {
                    if let std::net::IpAddr::V4(v4) = ipnet.ip() {
                        if v4.is_loopback() { continue; }
                        if is_private_ipv4(&v4) {
                            return Some((v4, mac.octets()));
                        }
                    }
                }
            }
        }

        // If we found a private IPv4 but the interface had no MAC, remember it to try mapping later.
        let fallback_ip = ifaces.iter().find_map(|iface| {
            for ipnet in &iface.ips {
                if let std::net::IpAddr::V4(v4) = ipnet.ip() {
                    if v4.is_loopback() { continue; }
                    if is_private_ipv4(&v4) { return Some(v4); }
                }
            }
            None
        });

        if let Some(ip) = fallback_ip {
            for iface in &ifaces {
                if let Some(mac) = iface.mac {
                    for ipnet in &iface.ips {
                        if let std::net::IpAddr::V4(v4) = ipnet.ip() {
                            if v4 == ip { return Some((ip, mac.octets())); }
                        }
                    }
                }
            }
        }
    }

    // Fallback: use existing UDP heuristic to infer the private IPv4, then map to an iface that has that IP.
    if let Some(ip) = get_private_ipv4() {
        use pnet_datalink::NetworkInterface;
        let ifaces: Vec<NetworkInterface> = pnet_datalink::interfaces();
        for iface in ifaces {
            if let Some(mac) = iface.mac {
                for ipnet in iface.ips {
                    if let std::net::IpAddr::V4(v4) = ipnet.ip() {
                        if v4 == ip {
                            return Some((ip, mac.octets()));
                        }
                    }
                }
            }
        }
    }

    None
}

/// Return a private (RFC1918) IPv4 address for this host if one can be inferred.
///
/// Strategy:
/// - Use the UDP "connect" trick to let the OS select a suitable outbound
///   interface without sending any packets. We then read the socket's local
///   address to learn the chosen interface's IP.
/// - Try several remote endpoints (public and private-range dummies) to cover
///   typical routing cases even when offline. If multiple interfaces exist,
///   whichever yields a private IPv4 first is returned.
/// - If nothing suitable is found, return `None`.
pub fn get_private_ipv4() -> Option<Ipv4Addr> {
    // Candidate remote endpoints. We don't actually send any data; connecting a UDP socket
    // is enough for the OS to select a local interface. We try a few well-known public DNS
    // IPs and a few RFC1918 addresses to coax selection even if offline.
    const CANDIDATES: &[&str] = &[
        // Public resolvers
        "8.8.8.8:80",
        "1.1.1.1:80",
        "9.9.9.9:80",
        // Private-range dummy addresses (often trigger selection of private iface even offline)
        "192.168.0.1:80",
        "172.16.0.1:80",
        "10.255.255.255:80",
    ];

    for ep in CANDIDATES {
        if let Ok(remote) = ep.parse::<SocketAddr>() {
            // Bind to 0.0.0.0:0 to let OS choose the source port/interface.
            if let Ok(sock) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
                // Ignore connect errors; even if the remote isn't reachable, connect() on UDP
                // generally just sets the default peer, which is enough to obtain local_addr.
                let _ = sock.connect(remote);
                if let Ok(local) = sock.local_addr() {
                    if let std::net::IpAddr::V4(v4) = local.ip() {
                        if is_private_ipv4(&v4) {
                            return Some(v4);
                        }
                    }
                }
            }
        }
    }

    None
}

/// Check if an IPv4 address is within the RFC1918 private ranges.
/// - 10.0.0.0/8
/// - 172.16.0.0/12
/// - 192.168.0.0/16
#[inline]
pub fn is_private_ipv4(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();
    match octets {
        [10, _, _, _] => true,                                 // 10.0.0.0/8
        [172, b, _, _] if (16..=31).contains(&b) => true,      // 172.16.0.0/12
        [192, 168, _, _] => true,                              // 192.168.0.0/16
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_private_ranges() {
        assert!(is_private_ipv4(&Ipv4Addr::new(10, 0, 0, 1)));
        assert!(is_private_ipv4(&Ipv4Addr::new(10, 255, 255, 254)));

        for b in 16..=31 {
            assert!(is_private_ipv4(&Ipv4Addr::new(172, b, 0, 1)), "172.{}.0.1 should be private", b);
        }
        assert!(!is_private_ipv4(&Ipv4Addr::new(172, 15, 0, 1)));
        assert!(!is_private_ipv4(&Ipv4Addr::new(172, 32, 0, 1)));

        assert!(is_private_ipv4(&Ipv4Addr::new(192, 168, 1, 10)));

        assert!(!is_private_ipv4(&Ipv4Addr::new(127, 0, 0, 1))); // loopback
        assert!(!is_private_ipv4(&Ipv4Addr::new(100, 64, 0, 1))); // CGNAT (100.64/10) not RFC1918
        assert!(!is_private_ipv4(&Ipv4Addr::new(8, 8, 8, 8)));   // public
    }

    #[test]
    fn get_private_ipv4_is_optional_and_valid() {
        // This environment-agnostic test only verifies that if an address is returned,
        // it indeed belongs to an RFC1918 range.
        if let Some(ip) = get_private_ipv4() {
            assert!(is_private_ipv4(&ip), "Returned IP must be private: {}", ip);
        }
    }

    #[test]
    fn get_private_ipv4_with_mac_optional_and_valid() {
        if let Some((ip, mac)) = get_private_ipv4_with_mac() {
            assert!(is_private_ipv4(&ip), "Returned IP must be private: {}", ip);
            assert_ne!(mac, [0u8; 6], "MAC should not be all zeros");
        }
    }
}
