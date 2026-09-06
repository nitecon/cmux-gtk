//! Bounded Linux TCP listener attribution for explicitly supplied process identities.
use std::{
    collections::HashSet,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::PathBuf,
};

/// A PID qualified by Linux start ticks; callers retain identity across asynchronous discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessIdentity {
    /// Linux process ID.
    pub pid: u32,
    /// Field 22 of procfs stat, relative to boot.
    pub start_ticks: u64,
}

/// One listening socket owned by the qualified process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    /// Process owning a descriptor for this socket.
    pub process: ProcessIdentity,
    /// Bind address as reported in the process's network namespace.
    pub address: IpAddr,
    /// TCP port in host byte order.
    pub port: u16,
    /// Kernel socket inode, used only for this observation.
    pub inode: u64,
}

/// Read a process identity without interpreting command names; worker-only bounded filesystem I/O.
pub fn identity(pid: u32) -> io::Result<ProcessIdentity> {
    let stat =
        crate::filesystem::read_text_bounded(&PathBuf::from(format!("/proc/{pid}/stat")), 8192)?;
    let mut fields = stat
        .rsplit_once(')')
        .ok_or_else(invalid)?
        .1
        .split_whitespace();
    let ticks = fields
        .nth(19)
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    Ok(ProcessIdentity {
        pid,
        start_ticks: ticks,
    })
}

/// Inspect up to 256 explicitly qualified processes, 4096 descriptors each and 2 MiB per TCP table.
/// Only LISTEN sockets with descriptors owned by the same unchanged process are returned.
/// Exited/reused processes are omitted; other I/O failures remain errors, not an empty successful scan.
/// No process discovery, launching, signalling, namespace changes or GTK work occurs here.
pub fn listening_tcp(processes: &[ProcessIdentity]) -> io::Result<Vec<Listener>> {
    if processes.len() > 256 {
        return Err(invalid());
    }
    let mut result = Vec::new();
    for process in processes {
        if !matches_identity(*process)? {
            continue;
        }
        let base = PathBuf::from(format!("/proc/{}", process.pid));
        let scan = || -> io::Result<Vec<Listener>> {
            let mut sockets = HashSet::new();
            for (count, entry) in std::fs::read_dir(base.join("fd"))?.enumerate() {
                if count >= 4096 {
                    return Err(invalid());
                }
                let path = entry?.path();
                let link = match std::fs::read_link(path) {
                    Ok(link) => link,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error),
                };
                if let Some(inode) = link
                    .to_str()
                    .and_then(|link| link.strip_prefix("socket:["))
                    .and_then(|link| link.strip_suffix(']'))
                    .and_then(|value| value.parse::<u64>().ok())
                {
                    sockets.insert(inode);
                }
            }
            let mut listeners = Vec::new();
            for table in ["tcp", "tcp6"] {
                let text = crate::filesystem::read_text_bounded(
                    &base.join("net").join(table),
                    2 * 1024 * 1024,
                )?;
                for line in text.lines().skip(1) {
                    if let Some((address, port, inode)) = parse_listener(line) {
                        if sockets.contains(&inode) {
                            listeners.push(Listener {
                                process: *process,
                                address,
                                port,
                                inode,
                            });
                        }
                    }
                }
            }
            Ok(listeners)
        };
        match scan() {
            Ok(listeners) if matches_identity(*process)? => result.extend(listeners),
            Ok(_) => {}
            Err(error)
                if error.kind() == io::ErrorKind::NotFound && !matches_identity(*process)? => {}
            Err(error) => return Err(error),
        }
    }
    Ok(result)
}

/// Reject PID reuse and tolerate exit while retaining permission and malformed-procfs errors.
fn matches_identity(process: ProcessIdentity) -> io::Result<bool> {
    match identity(process.pid) {
        Ok(current) => Ok(current == process),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Parse Linux TCP tables; address words use native byte order, ports use hexadecimal host values.
fn parse_listener(line: &str) -> Option<(IpAddr, u16, u64)> {
    let fields: Vec<_> = line.split_whitespace().take(11).collect();
    if fields.len() < 10 || fields[3] != "0A" {
        return None;
    }
    let (address, port) = fields[1].split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    let inode = fields[9].parse().ok()?;
    let address = match address.len() {
        8 => IpAddr::V4(Ipv4Addr::from(
            u32::from_str_radix(address, 16).ok()?.to_ne_bytes(),
        )),
        32 => {
            let mut bytes = [0u8; 16];
            for (word, output) in address
                .as_bytes()
                .chunks_exact(8)
                .zip(bytes.chunks_exact_mut(4))
            {
                output.copy_from_slice(
                    &u32::from_str_radix(std::str::from_utf8(word).ok()?, 16)
                        .ok()?
                        .to_ne_bytes(),
                );
            }
            IpAddr::V6(Ipv6Addr::from(bytes))
        }
        _ => return None,
    };
    Some((address, port, inode))
}

/// Fixed error text excludes process paths and network metadata.
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid or oversized process listener data",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real owned listener is attributed, a reused identity is rejected, and closing removes its record.
    #[test]
    fn attributes_owned_listener_and_rejects_reused_identity() {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let owner = identity(std::process::id()).unwrap();
        assert!(listening_tcp(&[owner])
            .unwrap()
            .iter()
            .any(|row| row.port == port && row.address == IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert!(listening_tcp(&[ProcessIdentity {
            start_ticks: owner.start_ticks + 1,
            ..owner
        }])
        .unwrap()
        .is_empty());
        drop(listener);
        assert!(!listening_tcp(&[owner])
            .unwrap()
            .iter()
            .any(|row| row.port == port));
    }
}
