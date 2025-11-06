// extv: read an ext2/3/4 image from a plain file, with no mount and no
// privilege, and check it for consistency.
//
// Exit status, kept distinct on purpose so the test harness can tell a
// refusal from a wrong answer:
//   0  the command answered
//   1  the image is inconsistent (`check` only)
//   2  the image is malformed and extv refuses to guess
//   3  usage error, or the file could not be read

use std::process::ExitCode;

const USAGE: &str = "\
usage: extv COMMAND IMAGE [ARG]

  super IMAGE            superblock fields, one per line
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        print!("{}", USAGE);
        return ExitCode::from(3);
    }
    eprintln!("extv: unknown command: {}", args[0]);
    ExitCode::from(3)
}
