//! A stand-in for a batfiles release binary, which the tests compile once and copy with a trailer
//! appended: `\nbatfiles-stand-in <target> <version>\n`. It reports that version for `version`,
//! and otherwise prints its target, version, and arguments, as `<target> <version> ran: <args>`,
//! exiting with the status `--exit <n>` among them names.

fn main() {
    const MARKER: &[u8] = b"\nbatfiles-stand-in ";
    let exe = std::env::current_exe().expect("its own path");
    let bytes = std::fs::read(&exe).expect("its own file");
    // The trailer is the last occurrence: this program holds the marker too.
    let at = bytes
        .windows(MARKER.len())
        .rposition(|window| window == MARKER)
        .expect("a trailer");
    let trailer = String::from_utf8_lossy(&bytes[at + MARKER.len()..]);
    let (target, version) = trailer
        .trim_end()
        .split_once(' ')
        .expect("a target and a version");
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("version") {
        println!("batfiles {version}");
    } else {
        println!("{target} {version} ran: {}", args.join(" "));
        if let Some(at) = args.iter().position(|arg| arg == "--exit") {
            std::process::exit(args[at + 1].parse().expect("a status"));
        }
    }
}
