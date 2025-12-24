#![allow(dead_code)]
use crate::err::Result;
use crate::img::{Img, V};
use crate::inode::Inode;
use crate::map::{check_block, Builder};
use crate::sb::Sb;

// The classic ext2 block map: twelve direct pointers, then one, two and
// three levels of indirection. Every ext2 file and every ext4 file
// without the extent flag is read this way.
//
// The order matters as much as the contents: an indirect block is
// reported before the blocks it points at, and a pointer of zero is a
// hole and is skipped rather than reported as block 0. debugfs does
// both, so a reader that reports holes or that reports an indirect
// block after its children disagrees on every sparse file.
pub fn walk(img: &Img, sb: &Sb, ino: &Inode, b: &mut Builder) -> Result<()> {
    let v = V::new(&ino.block, "inode block map");
    let per = sb.block_size as u64 / 4;
    let mut logical: u64 = 0;
    for i in 0..12 {
        let p = v.u32(i * 4)? as u64;
        if p != 0 {
            check_block(sb, p, "direct block pointer")?;
            b.data(logical, p, false)?;
        }
        logical += 1;
    }
    let ind = v.u32(48)? as u64;
    if ind != 0 {
        indirect(img, sb, b, ind, 1, &mut logical)?;
    } else {
        logical += per;
    }
    let dind = v.u32(52)? as u64;
    if dind != 0 {
        indirect(img, sb, b, dind, 2, &mut logical)?;
    } else {
        logical += per * per;
    }
    let tind = v.u32(56)? as u64;
    if tind != 0 {
        indirect(img, sb, b, tind, 3, &mut logical)?;
    }
    Ok(())
}

fn indirect(
    img: &Img,
    sb: &Sb,
    b: &mut Builder,
    block: u64,
    level: u32,
    logical: &mut u64,
) -> Result<()> {
    check_block(sb, block, "indirect block pointer")?;
    b.meta(["IND", "DIND", "TIND"][level as usize - 1], block)?;
    let buf = img.block(block)?;
    let v = V::new(&buf, "indirect block");
    let per = sb.block_size as u64 / 4;
    let span = per.pow(level - 1);
    for i in 0..per as usize {
        let p = v.u32(i * 4)? as u64;
        if p == 0 {
            *logical += span;
            continue;
        }
        if level == 1 {
            check_block(sb, p, "indirect block entry")?;
            b.data(*logical, p, false)?;
            *logical += 1;
        } else {
            indirect(img, sb, b, p, level - 1, logical)?;
        }
    }
    Ok(())
}
