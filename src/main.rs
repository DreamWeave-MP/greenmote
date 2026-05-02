fn main() -> std::io::Result<()> {
    match greenmote::run() {
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}
