// SPDX-License-Identifier: GPL-3.0-only

//! Windows gives a console program started from Explorer, a shortcut or a launcher a console
//! window of its own. Greenmote stays a console program so the command line keeps its output,
//! its prompts and its exit code; the GUI lets go of a console that is its own instead.

/// Closes the console window Windows opened for this process alone, so the GUI does not open
/// behind an empty terminal. A console shared with the shell greenmote was started from stays
/// attached.
#[cfg(windows)]
pub(super) fn release_own_console() {
    use windows_sys::Win32::System::Console::{FreeConsole, GetConsoleProcessList};

    let mut process_id = 0;
    // SAFETY: the list has room for one process id, and the count passed says so. A console with
    // more processes returns the count without writing past it.
    let attached_processes = unsafe { GetConsoleProcessList(&raw mut process_id, 1) };
    if owns_console_alone(attached_processes) {
        // SAFETY: FreeConsole takes nothing. Writes to std's handles for the released console
        // fail with ERROR_INVALID_HANDLE, which std reports as written.
        unsafe { FreeConsole() };
    }
}

/// `GetConsoleProcessList` counts every process attached to this one's console, this one
/// included, and returns 0 when there is no console.
const fn owns_console_alone(attached_processes: u32) -> bool {
    attached_processes == 1
}

#[cfg(test)]
mod tests {
    use super::owns_console_alone;

    #[test]
    fn only_a_console_no_other_process_shares_is_released() {
        assert!(owns_console_alone(1));
        assert!(!owns_console_alone(2));
        assert!(!owns_console_alone(3));
        assert!(!owns_console_alone(0));
    }
}
