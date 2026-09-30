// SPDX-License-Identifier: MIT OR Apache-2.0

use std::process::ExitCode;

fn main() -> ExitCode {
    match greenmote::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
