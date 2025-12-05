// extv: read an ext2/3/4 image from a plain file, with no mount and no
// privilege, and check it for consistency.
//
// Exit status, kept distinct on purpose so the test harness can tell a
// refusal from a wrong answer:
//   0  the command answered
//   1  the image is inconsistent (`check` only)
//   2  the image is malformed and extv refuses to guess
//   3  usage error, or the file could not be read

mod err;
mod group;
mod img;
mod sb;
mod show;

use err::Result;
use std::io::Write;
use std::process::ExitCode;

const USAGE: &str = "\
usage: extv COMMAND IMAGE [ARG]

  super IMAGE            superblock fields, one key and value per line
  groups IMAGE           every block group descriptor
";

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
    let known = ["super", "groups"];
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
