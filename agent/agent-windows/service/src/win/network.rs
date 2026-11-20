//! The machine's own addresses, offered to the browser as direct WebRTC candidates.

use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use windows_sys::Win32::{
    NetworkManagement::{
        IpHelper::{
            GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_FRIENDLY_NAME,
            GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
        },
        Ndis::IfOperStatusUp,
    },
    Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC},
};

const ERROR_BUFFER_OVERFLOW: u32 = 111;
/// IF_TYPE_SOFTWARE_LOOPBACK.
const LOOPBACK: u32 = 24;

/// Addresses that a peer on the network may reach directly: interfaces that are up, without
/// loopback, link-local or unspecified addresses.
pub fn local_addresses() -> io::Result<Vec<IpAddr>> {
    let flags = GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_DNS_SERVER
        | GAA_FLAG_SKIP_FRIENDLY_NAME;
    let mut size: u32 = 16 * 1024;
    let mut buffer: Vec<u64> = Vec::new();
    for _ in 0..4 {
        buffer = vec![0; (size as usize).div_ceil(8)];
        // SAFETY: the buffer is writable for `size` bytes and 8-byte aligned.
        let result = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_UNSPEC),
                flags,
                std::ptr::null(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        match result {
            0 => break,
            ERROR_BUFFER_OVERFLOW => continue,
            error => return Err(io::Error::from_raw_os_error(error as i32)),
        }
    }
    let mut addresses = Vec::new();
    let mut adapter = buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    // SAFETY: GetAdaptersAddresses filled the buffer with a linked list that lives inside it.
    unsafe {
        while !adapter.is_null() {
            let current = &*adapter;
            if current.OperStatus == IfOperStatusUp && current.IfType != LOOPBACK {
                let mut unicast = current.FirstUnicastAddress;
                while !unicast.is_null() {
                    let entry = &*unicast;
                    let socket = entry.Address;
                    if !socket.lpSockaddr.is_null() {
                        let bytes = std::slice::from_raw_parts(
                            socket.lpSockaddr.cast::<u8>(),
                            socket.iSockaddrLength.max(0) as usize,
                        );
                        if let Some(address) = parse(bytes) {
                            addresses.push(address);
                        }
                    }
                    unicast = entry.Next;
                }
            }
            adapter = current.Next;
        }
    }
    addresses.sort();
    addresses.dedup();
    Ok(addresses)
}

/// A socket address in its raw `SOCKADDR_IN` or `SOCKADDR_IN6` layout.
fn parse(bytes: &[u8]) -> Option<IpAddr> {
    let family = u16::from_le_bytes(bytes.get(..2)?.try_into().ok()?);
    let address = match family {
        AF_INET => IpAddr::V4(Ipv4Addr::from(<[u8; 4]>::try_from(bytes.get(4..8)?).ok()?)),
        AF_INET6 => IpAddr::V6(Ipv6Addr::from(
            <[u8; 16]>::try_from(bytes.get(8..24)?).ok()?,
        )),
        _ => return None,
    };
    let usable = match address {
        IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_link_local() && !v4.is_unspecified(),
        IpAddr::V6(v6) => {
            !v6.is_loopback() && !v6.is_unspecified() && (v6.segments()[0] & 0xffc0) != 0xfe80
        }
    };
    usable.then_some(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_addresses_parse_and_unreachable_ones_are_skipped() {
        let mut v4 = vec![0u8; 16];
        v4[..2].copy_from_slice(&AF_INET.to_le_bytes());
        v4[4..8].copy_from_slice(&[192, 168, 1, 20]);
        assert_eq!(parse(&v4), Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20))));
        v4[4..8].copy_from_slice(&[127, 0, 0, 1]);
        assert_eq!(parse(&v4), None);
        v4[4..8].copy_from_slice(&[169, 254, 3, 4]);
        assert_eq!(parse(&v4), None);
        let mut v6 = vec![0u8; 28];
        v6[..2].copy_from_slice(&AF_INET6.to_le_bytes());
        v6[8..24].copy_from_slice(&"2001:db8::5".parse::<Ipv6Addr>().unwrap().octets());
        assert_eq!(parse(&v6), Some("2001:db8::5".parse().unwrap()));
        v6[8..24].copy_from_slice(&"fe80::1".parse::<Ipv6Addr>().unwrap().octets());
        assert_eq!(parse(&v6), None);
        assert_eq!(parse(&v4[..6]), None);
        assert_eq!(parse(&[9, 0, 0, 0]), None);
    }

    #[test]
    fn this_machine_lists_its_addresses() {
        let addresses = local_addresses().unwrap();
        assert!(addresses.iter().all(|address| !address.is_loopback()));
    }
}
