use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Returns true if the IPv4 address belongs to one of the private,
/// link-local, or (optionally) CGNAT ranges.
pub fn is_private_network_address_v4(addr: Ipv4Addr, match_cgn: bool) -> bool {
    let value = u32::from(addr);

    // 10.0.0.0/8
    if (value & 0xff00_0000) == 0x0a00_0000 {
        return true;
    }

    // 172.16.0.0/12
    if (value & 0xfff0_0000) == 0xac10_0000 {
        return true;
    }

    // 192.168.0.0/16
    if (value & 0xffff_0000) == 0xc0a8_0000 {
        return true;
    }

    // 169.254.0.0/16
    if (value & 0xffff_0000) == 0xa9fe_0000 {
        return true;
    }

    // 100.64.0.0/10 - CGNAT
    if match_cgn && (value & 0xffc0_0000) == 0x6440_0000 {
        return true;
    }

    false
}

/// Returns true if an IPv6 address has the specified prefix.
///
/// `prefix_len` is in bits.
pub fn is_in_subnet_v6(addr: &Ipv6Addr, subnet: &[u8], prefix_len: usize) -> bool {
    let bytes = addr.octets();

    for i in 0..prefix_len {
        let byte_index = i / 8;
        let bit_mask = 1u8 << (i % 8);

        if (bytes[byte_index] & bit_mask) != (subnet[byte_index] & bit_mask) {
            return false;
        }
    }

    true
}

/// Returns true if the address is in one of the private/local ranges.
pub fn is_private_network_address(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(addr) => is_private_network_address_v4(addr, false),

        IpAddr::V6(addr) => {
            // fe80::/10 - link-local
            let link_local = [0xfe, 0x80];
            if is_in_subnet_v6(&addr, &link_local, 10) {
                return true;
            }

            // fec0::/10 - site-local (deprecated, but preserved from C code)
            let site_local = [0xfe, 0xc0];
            if is_in_subnet_v6(&addr, &site_local, 10) {
                return true;
            }

            // fc00::/7 - unique local address
            let unique_local = [0xfc, 0x00];

            is_in_subnet_v6(&addr, &unique_local, 7)
        }
    }
}
