use std::io;

mod args;

pub use args::GroundcoverArgs;

pub fn run(args: GroundcoverArgs) -> io::Result<()> {
    if args.debug {
        eprintln!("greenmote convert scaffold is wired; implementation follows.");
    }

    Ok(())
}
