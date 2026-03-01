#![allow(dead_code)]
use crate::err::{bad, Error, Result};
use std::fs::File;
use std::os::unix::fs::FileExt;

// The image is never slurped into one buffer. Every read allocates exactly
// as many bytes as it is going to use and reads them straight out of the
// file, so a read that walks off the end of a block lands outside its
// allocation and panics instead of quietly returning the next block's
// bytes. One image-sized buffer would make the whole class invisible; see
// `make overread`, and METHOD's note on the eighteen buffers found in the
// sibling repos.
pub struct Img {
    f: File,
    pub len: u64,
    pub bsize: u32,
    pub path: String,
}

impl Img {
    pub fn open(path: &str) -> Result<Img> {
        let f = File::open(path).map_err(|e| Error::Io(format!("{}: {}", path, e)))?;
        let md = f
            .metadata()
            .map_err(|e| Error::Io(format!("{}: {}", path, e)))?;
        if md.is_dir() {
            return Err(Error::Io(format!("{}: is a directory", path)));
        }
        Ok(Img {
            f,
            len: md.len(),
            bsize: 1024,
            path: path.to_string(),
        })
    }

    // Exactly n bytes at off, or an error. A short read at the end of a
    // truncated image is an error and not a zero-filled buffer: an image
    // that stops in the middle of a block is malformed, and padding it
    // would turn a truncation into a plausible wrong answer.
    pub fn read_at(&self, off: u64, n: usize) -> Result<Vec<u8>> {
        if n == 0 {
            return bad("zero-length read");
        }
        if off.checked_add(n as u64).is_none() || off + n as u64 > self.len {
            return bad(format!(
                "read of {} bytes at offset {} is past the end of the image ({} bytes)",
                n, off, self.len
            ));
        }
        let mut v = vec![0u8; n];
        self.f
            .read_exact_at(&mut v, off)
            .map_err(|e| Error::Io(format!("{}: read at {}: {}", self.path, off, e)))?;
        Ok(v)
    }

    pub fn block(&self, n: u64) -> Result<Vec<u8>> {
        let off = n
            .checked_mul(self.bsize as u64)
            .ok_or_else(|| Error::Malformed(format!("block number {} overflows", n)))?;
        let b = self.read_at(off, self.bsize as usize)?;
        #[cfg(feature = "plant_overread_block")]
        {
            // One byte past the end of a buffer that is exactly one block
            // long. Compiled in only by `make overread`.
            println!("plant: block byte {} = {}", self.bsize, b[self.bsize as usize]);
        }
        Ok(b)
    }
}

// Every multi-byte field in the image is read through this. ext2/3/4 is
// little-endian on disk whatever the host is, and every accessor is bounds
// checked against the length of the slice it was handed, so a structure
// that claims to extend past its block is a refusal rather than a panic.
pub struct V<'a> {
    pub b: &'a [u8],
    pub what: &'static str,
}

impl<'a> V<'a> {
    pub fn new(b: &'a [u8], what: &'static str) -> V<'a> {
        V { b, what }
    }

    pub fn bytes(&self, o: usize, n: usize) -> Result<&'a [u8]> {
        // checked_add, because o comes out of the image in the variable
        // length structures (dirents, xattrs) and o + n wrapping would turn
        // a bounds check into a pass.
        let end = match o.checked_add(n) {
            Some(e) => e,
            None => return bad(format!("{}: offset {} plus {} overflows", self.what, o, n)),
        };
        match self.b.get(o..end) {
            Some(s) => Ok(s),
            None => bad(format!(
                "{}: {} bytes at offset {} is past the end of a {}-byte structure",
                self.what,
                n,
                o,
                self.b.len()
            )),
        }
    }

    pub fn u8(&self, o: usize) -> Result<u8> {
        Ok(self.bytes(o, 1)?[0])
    }

    pub fn u16(&self, o: usize) -> Result<u16> {
        let s = self.bytes(o, 2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }

    pub fn u32(&self, o: usize) -> Result<u32> {
        let s = self.bytes(o, 4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    // JBD2 is big-endian, alone in an image that is little-endian
    // everywhere else, so this accessor exists for the journal and for
    // nothing else.
    pub fn u32be(&self, o: usize) -> Result<u32> {
        let s = self.bytes(o, 4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }

    pub fn u64(&self, o: usize) -> Result<u64> {
        let s = self.bytes(o, 8)?;
        Ok(u64::from_le_bytes([
            s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
        ]))
    }

    // A 32-bit field split into a low and a high half by the 64bit feature,
    // which is how every block pointer in a group descriptor is stored.
    pub fn u32_pair(&self, lo: usize, hi: usize, wide: bool) -> Result<u64> {
        let l = self.u32(lo)? as u64;
        if wide {
            Ok(l | ((self.u32(hi)? as u64) << 32))
        } else {
            Ok(l)
        }
    }

    pub fn u16_pair(&self, lo: usize, hi: usize, wide: bool) -> Result<u32> {
        let l = self.u16(lo)? as u32;
        if wide {
            Ok(l | ((self.u16(hi)? as u32) << 16))
        } else {
            Ok(l)
        }
    }
}
