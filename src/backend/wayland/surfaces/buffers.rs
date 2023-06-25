use kurbo::{Rect, Size};
use nix::sys::mman::{mmap, munmap, MapFlags, ProtFlags};
use std::{
    convert::TryInto,
    fmt,
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
                log::warn!("Error unmapping memory: {}", e);
            }
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct RawSize {
    pub width: i32,
    pub height: i32,
}

impl RawSize {
    pub const ZERO: Self = Self {
        width: 0,
        height: 0,
    };

    pub fn scale(self, scale: i32) -> Self {
        // NOTE no overflow checking atm.
        RawSize {
            width: self.width * scale,
            height: self.height * scale,
        }
    }

    pub fn to_rect(self) -> RawRect {
        RawRect {
            x0: 0,
            y0: 0,
            x1: self.width,
            y1: self.height,
        }
    }

    pub fn area(self) -> i32 {
        self.width * self.height
    }

    pub fn is_empty(self) -> bool {
        self.area() == 0
    }
}

impl From<Size> for RawSize {
    fn from(s: Size) -> Self {
        let width = s.width as i32;
        let height = s.height as i32;
        // Sanity check
        assert!(width >= 0 && height >= 0);

        RawSize { width, height }
    }
}

impl From<RawSize> for Size {
    fn from(s: RawSize) -> Self {
        Size::new(s.width as f64, s.height as f64)
    }
}

impl fmt::Debug for RawSize {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}×{}", self.width, self.height)
    }
}

#[derive(Debug)]
pub struct RawRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl RawRect {
    pub fn scale(self, scale: i32) -> Self {
        // NOTE no overflow checking atm.
        RawRect {
            x0: self.x0 * scale,
            y0: self.y0 * scale,
            x1: self.x1 * scale,
            y1: self.y1 * scale,
        }
    }
}

impl From<Rect> for RawRect {
    fn from(r: Rect) -> Self {
        let max = i32::MAX as f64;
        let r = r.expand();
        assert!(r.x0.abs() < max && r.y0.abs() < max && r.x1.abs() < max && r.y1.abs() < max);
        RawRect {
            x0: r.x0 as i32,
            y0: r.y0 as i32,
            x1: r.x1 as i32,
            y1: r.y1 as i32,
        }
    }
}

impl From<RawRect> for Rect {
    fn from(r: RawRect) -> Self {
        Rect {
            x0: r.x0 as f64,
            y0: r.y0 as f64,
            x1: r.x1 as f64,
            y1: r.y1 as f64,
        }
    }
}
