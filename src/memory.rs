//! Bounded read-only Linux reads. No scanning or game profiles.
use crate::process::{self, Identity, Mapping};
use std::{fmt, io};
pub const MAX_READ_BYTES: usize = 4096;
pub const MAX_SCATTER_RANGES: usize = 128;
pub const MAX_SCATTER_BYTES: usize = 65536;
#[derive(Debug)]
pub enum ReadError {
    InvalidRequest,
    UnreadableRange,
    IdentityChanged,
    Metadata(io::Error),
    System(io::Error),
    Partial { requested: usize, received: usize },
}
impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest => write!(f, "invalid or oversized read request"),
            Self::UnreadableRange => write!(f, "request is outside a readable mapping"),
            Self::IdentityChanged => write!(f, "process exited or identity changed"),
            Self::Metadata(e) => write!(f, "process metadata: {e}"),
            Self::System(e) => write!(f, "read access: {e}"),
            Self::Partial {
                requested,
                received,
            } => write!(
                f,
                "partial read: {received} of {requested} bytes; data discarded"
            ),
        }
    }
}
impl std::error::Error for ReadError {}
fn validate(address: usize, len: usize, maps: &[Mapping]) -> Result<(), ReadError> {
    if len == 0 || len > MAX_READ_BYTES || address == 0 {
        return Err(ReadError::InvalidRequest);
    }
    let end = address.checked_add(len).ok_or(ReadError::InvalidRequest)?;
    if !maps
        .iter()
        .any(|m| m.readable && address >= m.start && end <= m.end)
    {
        return Err(ReadError::UnreadableRange);
    }
    Ok(())
}
fn finish(result: io::Result<usize>, len: usize, identity_matches: bool) -> Result<(), ReadError> {
    if !identity_matches {
        return Err(ReadError::IdentityChanged);
    }
    match result {
        Err(e) => Err(ReadError::System(e)),
        Ok(n) if n != len => Err(ReadError::Partial {
            requested: len,
            received: n,
        }),
        Ok(_) => Ok(()),
    }
}
pub fn read_exact(identity: &Identity, address: usize, len: usize) -> Result<Vec<u8>, ReadError> {
    if process::identity(identity.pid).ok().as_ref() != Some(identity) {
        return Err(ReadError::IdentityChanged);
    }
    let maps = process::mappings(identity.pid).map_err(ReadError::Metadata)?;
    validate(address, len, &maps)?;
    let mut bytes = vec![0; len];
    let local = libc::iovec {
        iov_base: bytes.as_mut_ptr().cast(),
        iov_len: len,
    };
    let remote = libc::iovec {
        iov_base: address as *mut libc::c_void,
        iov_len: len,
    };
    // SAFETY: the local iovec references an allocated len-byte buffer, both iovec
    // arrays have one valid element, and the remote range/size was checked above.
    // The kernel handles foreign pointers; no remote pointer is dereferenced here.
    let count = unsafe { libc::process_vm_readv(identity.pid, &local, 1, &remote, 1, 0) };
    let result = if count < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(count as usize)
    };
    let matches = process::identity(identity.pid).ok().as_ref() == Some(identity);
    finish(result, len, matches)?;
    Ok(bytes)
}
fn scatter_size(ranges: &[(usize, usize)]) -> Result<usize, ReadError> {
    if ranges.is_empty() || ranges.len() > MAX_SCATTER_RANGES {
        return Err(ReadError::InvalidRequest);
    }
    let mut total = 0usize;
    for &(address, len) in ranges {
        if address == 0 || len == 0 || len > MAX_READ_BYTES || address.checked_add(len).is_none() {
            return Err(ReadError::InvalidRequest);
        }
        total = total.checked_add(len).ok_or(ReadError::InvalidRequest)?;
        if total > MAX_SCATTER_BYTES {
            return Err(ReadError::InvalidRequest);
        }
    }
    Ok(total)
}
fn finish_scatter(
    result: io::Result<usize>,
    total: usize,
    identity_matches: bool,
    buffers: Vec<Vec<u8>>,
) -> Result<Vec<Vec<u8>>, ReadError> {
    finish(result, total, identity_matches)?;
    Ok(buffers)
}
/// Read at most 128 separately bounded ranges in one read-only syscall.
/// Fresh mappings validate every range. A partial batch or changed PID discards
/// every buffer, including ranges the kernel may have copied successfully.
pub fn read_scatter(
    identity: &Identity,
    ranges: &[(usize, usize)],
) -> Result<Vec<Vec<u8>>, ReadError> {
    let total = scatter_size(ranges)?;
    if process::identity(identity.pid).ok().as_ref() != Some(identity) {
        return Err(ReadError::IdentityChanged);
    }
    let maps = process::mappings(identity.pid).map_err(ReadError::Metadata)?;
    for &(address, len) in ranges {
        validate(address, len, &maps)?;
    }
    let mut buffers: Vec<Vec<u8>> = ranges.iter().map(|(_, len)| vec![0; *len]).collect();
    let local: Vec<libc::iovec> = buffers
        .iter_mut()
        .map(|bytes| libc::iovec {
            iov_base: bytes.as_mut_ptr().cast(),
            iov_len: bytes.len(),
        })
        .collect();
    let remote: Vec<libc::iovec> = ranges
        .iter()
        .map(|(address, len)| libc::iovec {
            iov_base: *address as *mut libc::c_void,
            iov_len: *len,
        })
        .collect();
    // SAFETY: each local iovec points at a distinct allocated buffer which is
    // neither resized nor moved during the call. Both arrays contain the same
    // bounded number of valid iovecs. All foreign ranges were checked against
    // fresh readable mappings; only the kernel dereferences remote addresses.
    let count = unsafe {
        libc::process_vm_readv(
            identity.pid,
            local.as_ptr(),
            local.len() as _,
            remote.as_ptr(),
            remote.len() as _,
            0,
        )
    };
    let result = if count < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(count as usize)
    };
    let matches = process::identity(identity.pid).ok().as_ref() == Some(identity);
    finish_scatter(result, total, matches, buffers)
}

/// Larger read path exclusively for bounded discovery. Callers periodically
/// revalidate the exact mapping; PID identity is checked around every read.
pub(crate) fn read_discovery(
    identity: &Identity,
    mapping: &Mapping,
    address: usize,
    len: usize,
) -> Result<Vec<u8>, ReadError> {
    if len == 0 || len > 65536 || address == 0 {
        return Err(ReadError::InvalidRequest);
    }
    let end = address.checked_add(len).ok_or(ReadError::InvalidRequest)?;
    if !mapping.readable || address < mapping.start || end > mapping.end {
        return Err(ReadError::UnreadableRange);
    }
    if process::identity(identity.pid).ok().as_ref() != Some(identity) {
        return Err(ReadError::IdentityChanged);
    }
    let mut bytes = vec![0; len];
    let local = libc::iovec {
        iov_base: bytes.as_mut_ptr().cast(),
        iov_len: len,
    };
    let remote = libc::iovec {
        iov_base: address as *mut libc::c_void,
        iov_len: len,
    };
    // SAFETY: one valid local allocated buffer and bounded remote range. The
    // kernel reads foreign memory; this code never dereferences remote pointers.
    let count = unsafe { libc::process_vm_readv(identity.pid, &local, 1, &remote, 1, 0) };
    let result = if count < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(count as usize)
    };
    finish(
        result,
        len,
        process::identity(identity.pid).ok().as_ref() == Some(identity),
    )?;
    Ok(bytes)
}
pub fn self_test() -> Result<(), ReadError> {
    let fixture = [0x51u8, 0xa2, 0x13, 0x74];
    let identity = process::identity(std::process::id() as i32).map_err(ReadError::Metadata)?;
    let bytes = read_exact(&identity, fixture.as_ptr() as usize, fixture.len())?;
    if bytes != fixture {
        return Err(ReadError::InvalidRequest);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn map() -> Mapping {
        Mapping {
            start: 10,
            end: 100,
            readable: true,
            writable: true,
            executable: false,
            offset: 0,
            device: "00:00".into(),
            inode: 0,
            path: None,
        }
    }
    #[test]
    fn synthetic_scatter_preserves_range_order() {
        let first = [1u8, 2, 3];
        let second = [4u8, 5];
        let identity = process::identity(std::process::id() as i32).unwrap();
        let ranges = [
            (second.as_ptr() as usize, second.len()),
            (first.as_ptr() as usize, first.len()),
        ];
        assert_eq!(
            read_scatter(&identity, &ranges).unwrap(),
            vec![second.to_vec(), first.to_vec()]
        );
    }
    #[test]
    fn scatter_bounds_ranges_and_reused_pid_fail_closed() {
        let identity = process::identity(std::process::id() as i32).unwrap();
        assert_eq!(
            scatter_size(&vec![(1, 1); MAX_SCATTER_RANGES]).unwrap(),
            MAX_SCATTER_RANGES
        );
        assert_eq!(
            scatter_size(&[(1, MAX_READ_BYTES); 16]).unwrap(),
            MAX_SCATTER_BYTES
        );
        for ranges in [
            vec![],
            vec![(1, 0)],
            vec![(0, 1)],
            vec![(usize::MAX, 2)],
            vec![(1, 4097)],
            vec![(1, 1); 129],
            vec![(1, 4096); 17],
        ] {
            assert!(matches!(
                read_scatter(&identity, &ranges),
                Err(ReadError::InvalidRequest)
            ));
        }
        assert!(matches!(
            read_scatter(&identity, &[(1, 4)]),
            Err(ReadError::UnreadableRange)
        ));
        let mut reused = identity;
        reused.start_ticks += 1;
        assert!(matches!(
            read_scatter(&reused, &[(1, 4)]),
            Err(ReadError::IdentityChanged)
        ));
    }
    #[test]
    fn partial_scatter_discards_even_completed_buffers() {
        assert!(matches!(
            finish_scatter(Ok(4), 8, true, vec![vec![1; 4], vec![0; 4]]),
            Err(ReadError::Partial {
                requested: 8,
                received: 4
            })
        ));
        assert!(matches!(
            finish_scatter(Ok(8), 8, false, vec![vec![1; 4], vec![2; 4]]),
            Err(ReadError::IdentityChanged)
        ));
        assert!(finish_scatter(Ok(8), 8, true, vec![vec![1; 4], vec![2; 4]]).is_ok());
    }
    #[test]
    fn request_boundaries() {
        assert!(validate(10, 90, &[map()]).is_ok());
        for (a, n) in [(0, 1), (10, 0), (10, MAX_READ_BYTES + 1), (usize::MAX, 2)] {
            assert!(matches!(
                validate(a, n, &[map()]),
                Err(ReadError::InvalidRequest)
            ));
        }
        assert!(matches!(
            validate(99, 2, &[map()]),
            Err(ReadError::UnreadableRange)
        ));
        let mut m = map();
        m.readable = false;
        assert!(validate(10, 1, &[m]).is_err());
    }
    #[test]
    fn partial_and_identity_discard() {
        assert!(matches!(
            finish(Ok(2), 4, true),
            Err(ReadError::Partial { received: 2, .. })
        ));
        assert!(matches!(
            finish(Ok(4), 4, false),
            Err(ReadError::IdentityChanged)
        ));
        assert!(matches!(
            finish(Err(io::Error::from_raw_os_error(libc::EPERM)), 4, false),
            Err(ReadError::IdentityChanged)
        ));
        assert!(matches!(
            finish(Err(io::Error::from_raw_os_error(libc::EPERM)), 4, true),
            Err(ReadError::System(_))
        ));
    }
    #[test]
    fn synthetic_self_read() {
        self_test().unwrap();
    }
    #[test]
    fn discovery_bounds_checked_before_remote_access() {
        let identity = Identity {
            pid: -1,
            start_ticks: 0,
        };
        let m = map();
        for (address, len) in [(10, 0), (10, 65537), (usize::MAX, 2)] {
            assert!(matches!(
                read_discovery(&identity, &m, address, len),
                Err(ReadError::InvalidRequest)
            ));
        }
        assert!(matches!(
            read_discovery(&identity, &m, 99, 2),
            Err(ReadError::UnreadableRange)
        ));
    }
    #[test]
    fn reused_pid_rejected() {
        let mut id = process::identity(std::process::id() as i32).unwrap();
        id.start_ticks += 1;
        assert!(matches!(
            read_exact(&id, 1, 1),
            Err(ReadError::IdentityChanged)
        ));
    }
}
