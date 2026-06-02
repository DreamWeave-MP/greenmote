// SPDX-License-Identifier: GPL-3.0-only

fn main() -> std::io::Result<()> {
    match greenmote::run() {
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}
