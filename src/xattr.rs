#![allow(dead_code)]
use crate::err::{bad, Result};
use crate::img::{Img, V};
use crate::inode::Inode;
use crate::sb::Sb;

pub const MAGIC: u32 = 0xEA02_0000;
pub const ENTRY_SIZE: usize = 16;

// The name is stored with its prefix replaced by a one-byte index.
// e2fsprogs keeps the same table, and two of the entries are whole
// names rather than prefixes.
pub const PREFIXES: &[(u8, &str)] = &[
    (1, "user."),
    (2, "system.posix_acl_access"),
    (3, "system.posix_acl_default"),
    (4, "trusted."),
    (6, "security."),
    (7, "system."),
    (8, "system.richacl"),
];

pub struct Attr {
    pub index: u8,
    pub name: Vec<u8>,
    pub value: Vec<u8>,
    pub value_inum: u32,
    pub hash: u32,
    // "inode" or "block": which of the two places it was found in.
    pub source: &'static str,
}

impl Attr {
    pub fn full_name(&self) -> String {
        let prefix = PREFIXES
            .iter()
            .find(|(i, _)| *i == self.index)
            .map(|(_, p)| *p)
            .unwrap_or("");
        format!("{}{}", prefix, String::from_utf8_lossy(&self.name))
    }
}

// One run of entries. The entries grow forwards from the start of the
// area and the values grow backwards from its end, so a value offset is
// relative to a base that differs between the two places attributes
// live: the start of the block in one case, the first entry in the
// other. Getting that base wrong reads a value from inside the entry
// table and prints something that looks like a name.
fn parse_entries(
    area: &[u8],
    entries_at: usize,
    value_base: usize,
    source: &'static str,
    out: &mut Vec<Attr>,
) -> Result<()> {
    let v = V::new(area, "extended attribute area");
    let mut off = entries_at;
    loop {
        if off + 4 > area.len() {
            return bad("extended attribute entries run past the end of their area");
        }
        if v.u32(off)? == 0 {
            return Ok(());
        }
        if off + ENTRY_SIZE > area.len() {
            return bad("extended attribute entry header runs past the end of its area");
        }
        let name_len = v.u8(off)? as usize;
        let index = v.u8(off + 1)?;
        let value_offs = v.u16(off + 2)? as usize;
        let value_inum = v.u32(off + 4)?;
        let value_size = v.u32(off + 8)? as usize;
        let hash = v.u32(off + 12)?;
        let name = v.bytes(off + ENTRY_SIZE, name_len)?.to_vec();
        let value = if value_inum != 0 {
            // ea_inode: the value is the contents of another inode.
            // Nothing in the corpus has one and extv does not follow
            // them, so the value is reported as absent rather than
            // guessed at.
            Vec::new()
        } else {
            let start = value_base + value_offs;
            match area.get(start..start + value_size) {
                Some(s) => s.to_vec(),
                None => {
                    return bad(format!(
                        "extended attribute value at {} for {} bytes is outside its area",
                        start, value_size
                    ))
                }
            }
        };
        out.push(Attr {
            index,
            name,
            value,
            value_inum,
            hash,
            source,
        });
        // Entries are padded to a multiple of four.
        off += (ENTRY_SIZE + name_len + 3) & !3;
    }
}

pub fn read(img: &Img, sb: &Sb, ino: &Inode) -> Result<Vec<Attr>> {
    let mut out = Vec::new();
    // In the inode, after the fixed 128 bytes and whatever extra fields
    // the inode declares.
    if sb.inode_size > 128 && ino.extra_isize >= 4 {
        let start = 128 + ino.extra_isize as usize;
        if start + 4 <= ino.raw.len() {
            let v = V::new(&ino.raw, "inode");
            if v.u32(start)? == MAGIC {
                parse_entries(&ino.raw, start + 4, start + 4, "inode", &mut out)?;
            }
        }
    }
    if ino.file_acl != 0 {
        crate::map::check_block(sb, ino.file_acl, "extended attribute block")?;
        let buf = img.block(ino.file_acl)?;
        let v = V::new(&buf, "extended attribute block");
        if v.u32(0)? != MAGIC {
            return bad(format!(
                "extended attribute block {} has magic 0x{:08x}, not 0x{:08x}",
                ino.file_acl,
                v.u32(0)?,
                MAGIC
            ));
        }
        parse_entries(&buf, 32, 0, "block", &mut out)?;
    }
    Ok(out)
}

// The attribute that holds the rest of an inline_data file or
// directory, when it does not all fit in the inode.
pub fn inline_tail(attrs: &[Attr]) -> Option<&Attr> {
    attrs
        .iter()
        .find(|a| a.index == 7 && a.name == b"data")
}

// The inline contents of an inode: the sixty bytes of i_block, then
// the system.data attribute holding whatever did not fit. The two are
// one contiguous area, so they are joined before anything parses
// them, and both callers wanted the same join.
//
// i_size comes off the image and nothing else bounds it. A size
// larger than the area asks for bytes that are not in it, and the
// caller used to hand that number to Vec::truncate, which only
// shrinks, so the whole attribute value went out as file content.
// The contents of an inline inode cannot be longer than the inline
// area, so a size that says otherwise is a corrupt inode and this
// refuses rather than guessing which part of it was meant.
pub fn inline_data(img: &Img, sb: &Sb, ino: &Inode) -> Result<Vec<u8>> {
    let attrs = read(img, sb, ino)?;
    let mut buf = ino.block.to_vec();
    if let Some(tail) = inline_tail(&attrs) {
        buf.extend_from_slice(&tail.value);
    }
    if ino.size as usize > buf.len() {
        return bad(format!(
            "inode {} holds its data inline and declares a size of {}, but \
             only {} bytes are inline",
            ino.num, ino.size, buf.len()
        ));
    }
    buf.truncate(ino.size as usize);
    Ok(buf)
}
