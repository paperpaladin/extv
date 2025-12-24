// extv: read an ext2/3/4 image from a plain file, with no mount and no
// privilege, and check it for consistency.
//
// Exit status, kept distinct on purpose so the test harness can tell a
// refusal from a wrong answer:
//   0  the command answered
//   1  the image is inconsistent (`check` only)
//   2  the image is malformed and extv refuses to guess
//   3  usage error, or the file could not be read

mod crc;
mod csum;
mod blockmap;
mod err;
mod extent;
mod group;
mod img;
mod inode;
mod map;
mod sb;
mod show;

use err::Result;
use std::io::Write;
use std::process::ExitCode;

const USAGE: &str = "\
usage: extv COMMAND IMAGE [ARG]

  super IMAGE            superblock fields, one key and value per line
  groups IMAGE           every block group descriptor
  csum IMAGE             recompute the metadata checksums
  stat IMAGE INODE[-N]   one inode, or a range of them, field by field
  map IMAGE INODE        the blocks of an inode, with the metadata blocks
  blocks IMAGE INODE     the same, as a flat list of physical blocks
  extents IMAGE INODE    the extent tree, one row per node entry
";

// Commands that take an inode number accept it bare or in the angle
// bracket form debugfs uses, so a command line can be copied from one
// tool to the other.
fn number(a: Option<&String>) -> Result<u64> {
    let s = match a {
        Some(s) => s.trim_start_matches('<').trim_end_matches('>'),
        None => return Err(err::Error::Io("an inode number is required".into())),
    };
    let (radix, digits) = if let Some(h) = s.strip_prefix("0x") {
        (16, h)
    } else {
        (10, s)
    };
    u64::from_str_radix(digits, radix)
        .map_err(|_| err::Error::Io(format!("{}: not a number", s)))
}

// An inode number or an inclusive range of them. The range form exists
// for the test harness: comparing six hundred inodes against debugfs
// one process at a time is slower than everything else in the suite put
// together.
fn range(a: Option<&String>) -> Result<(u64, u64)> {
    let s = match a {
        Some(s) => s.as_str(),
        None => return Err(err::Error::Io("an inode number is required".into())),
    };
    match s.split_once('-') {
        Some((f, l)) => Ok((
            number(Some(&f.to_string()))?,
            number(Some(&l.to_string()))?,
        )),
        None => {
            let n = number(Some(&s.to_string()))?;
            Ok((n, n))
        }
    }
}

fn run(args: &[String]) -> Result<i32> {
    let image = &args[1];
    let mut i = img::Img::open(image)?;
    let sb = sb::Sb::read(&i)?;
    i.bsize = sb.block_size;
    // One buffered writer for everything. Unbuffered printing costs a
    // write(2) per line, and the block list of a large file is hundreds
    // of thousands of lines.
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::new(stdout.lock());
    match args[0].as_str() {
        "super" => show::super_block(&mut w, &i, &sb)?,
        "groups" => show::groups(&mut w, &i, &sb)?,
        "csum" => show::checksums(&mut w, &i, &sb)?,
        "extents" => {
            let (first, last) = range(args.get(2))?;
            for n in first..=last {
                show::extents(&mut w, &i, &sb, n as u32)?;
            }
        }
        "map" | "blocks" => {
            let (first, last) = range(args.get(2))?;
            for n in first..=last {
                show::blockmap(&mut w, &i, &sb, n as u32, args[0] == "blocks")?;
            }
        }
        "stat" => {
            let (first, last) = range(args.get(2))?;
            for n in first..=last {
                show::stat(&mut w, &i, &sb, n as u32)?;
            }
        }
        _ => unreachable!(),
    }
    w.flush()?;
    Ok(0)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        print!("{}", USAGE);
        return ExitCode::from(3);
    }
    let known = ["super", "groups", "csum", "stat", "map", "blocks", "extents"];
    if !known.contains(&args[0].as_str()) {
        eprintln!("extv: unknown command: {}", args[0]);
        return ExitCode::from(3);
    }
    if args.len() < 2 {
        eprintln!("extv: {}: no image given", args[0]);
        return ExitCode::from(3);
    }
    match run(&args) {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            eprintln!("extv: {}", e);
            ExitCode::from(e.exit_code())
        }
    }
}
