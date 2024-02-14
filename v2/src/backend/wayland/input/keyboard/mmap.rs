//! Temporary implementation of memory mapping, to allow testing keyboard interaction
use nix::sys::mman::{mmap, munmap, MapFlags, ProtFlags};
use std::{
    convert::TryInto,
    ops::{Deref, DerefMut},
    os::{raw::c_void, unix::prelude::RawFd},
    ptr::{self, NonNull},
    slice,
};
pub struct Mmap {
    ptr: NonNull<c_void>,
    size: usize,
    offset: usize,
    len: usize,
}

impl Mmap {
    /// `fd` and `size` are the whole memory you want to map. `offset` and `len` are there to
    /// provide extra protection (only giving you access to that part).
    ///
    /// # Safety
    ///
    /// Concurrent use of the memory we map to isn't checked.
    #[inline]
    pub unsafe fn from_raw_private(
        fd: RawFd,
        size: usize,
        offset: usize,
        len: usize,
    ) -> Result<Self, nix::Error> {
        Self::from_raw_inner(fd, size, offset, len, true)
    }

    unsafe fn from_raw_inner(
        fd: RawFd,
        size: usize,
        offset: usize,
        len: usize,
        private: bool,
    ) -> Result<Self, nix::Error> {
        assert!(offset + len <= size, "{offset} + {len} <= {size}",);
        let map_flags = if private {
            MapFlags::MAP_PRIVATE
        } else {
            MapFlags::MAP_SHARED
        };
        let ptr = mmap(
            ptr::null_mut(),
            size,
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
            map_flags,
            fd,
            0,
        )?;
        Ok(Mmap {
            ptr: NonNull::new(ptr).unwrap(),
            size,
            offset,
            len,
        })
    }
}

impl Deref for Mmap {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            let start = self.ptr.as_ptr().offset(self.offset.try_into().unwrap());
            slice::from_raw_parts(start as *const u8, self.len)
        }
    }
}

impl DerefMut for Mmap {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            let start = self.ptr.as_ptr().offset(self.offset.try_into().unwrap());
            slice::from_raw_parts_mut(start as *mut u8, self.len)
        }
    }
}

impl Drop for Mmap {
    fn drop(&mut self) {
        unsafe {
            if let Err(e) = munmap(self.ptr.as_ptr(), self.size) {
                tracing::warn!("Error unmapping memory: {}", e);
            }
        }
    }
}
