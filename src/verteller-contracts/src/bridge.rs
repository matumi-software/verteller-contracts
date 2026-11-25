//! Starting the app, and the address it is found at.
//!
//! `verteller open`, `verteller stop` and `verteller mcp` all begin the same way: read the `port`
//! and `token` files in the data folder and ask the app whether it answers. When it is not running
//! a [`Launcher`] starts it and the caller waits for `/api/v1/system` to answer. The reading of the
//! data folder and the asking stay in the app, which owns the folder's rights ([`Endpoint`] is what
//! they find).

use std::{io, time::Duration};

/// How long the bridge waits for a freshly started app to answer.
pub const START_WAIT: Duration = Duration::from_secs(20);

/// How often it asks while waiting.
pub const START_POLL: Duration = Duration::from_millis(250);

/// Starts the app. The real one starts this same binary; a test stands in for it.
pub trait Launcher: Send + Sync {
    /// Starts the app and returns at once, without waiting for it to listen.
    fn launch(&self) -> io::Result<()>;
}

/// Starts this binary with no arguments, detached from the bridge and without a console window.
/// `data` is handed over in `VERTELLER_DATA` so the app uses the folder the bridge looked in.
///
/// On Windows the app also leaves the job object the starter is in, when there is one: a launcher,
/// an installer or a CI step may run `verteller open` in a job that ends every process in it when it
/// closes, and the app must outlive that. A job that does not allow it refuses the start, and then
/// the app is started inside the job after all (see [`spawn_breaking_away`]).
pub struct SpawnSelf {
    /// The data folder the bridge is using.
    pub data: std::path::PathBuf,
}

impl Launcher for SpawnSelf {
    fn launch(&self) -> io::Result<()> {
        use std::process::{Command, Stdio};
        let mut cmd = Command::new(std::env::current_exe()?);
        cmd.env("VERTELLER_DATA", &self.data)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            keep_stdio_to_ourselves();
            // The child is not waited for: it outlives the bridge on purpose.
            spawn_breaking_away(
                DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW,
                |flags| {
                    cmd.creation_flags(flags);
                    cmd.spawn().map(drop)
                },
            )
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
            // The child is not waited for: it outlives the bridge on purpose.
            cmd.spawn().map(drop)
        }
    }
}

/// `CreateProcess` flag: the child is not part of the job object its parent is in.
#[cfg(windows)]
pub const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

/// Calls `spawn` with `flags` and [`CREATE_BREAKAWAY_FROM_JOB`], and once more with `flags` alone
/// when that is refused with access denied, which is what a job that does not allow breaking away
/// answers. Outside a job the flag does nothing. Any other failure is returned as it is.
#[cfg(windows)]
pub fn spawn_breaking_away(
    flags: u32,
    mut spawn: impl FnMut(u32) -> io::Result<()>,
) -> io::Result<()> {
    const ERROR_ACCESS_DENIED: i32 = 5;
    match spawn(flags | CREATE_BREAKAWAY_FROM_JOB) {
        Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) => {
            tracing::info!("the job this runs in keeps its children, so the app starts inside it");
            spawn(flags)
        }
        other => other,
    }
}

/// A child gets every inheritable handle its parent holds, so the app would keep the bridge's stdout
/// pipe open after the bridge exits and the client would wait for it to close. Marking the three
/// standard handles as not inheritable keeps them out of the app. The bridge still uses them.
#[cfg(windows)]
fn keep_stdio_to_ourselves() {
    use windows_sys::Win32::{
        Foundation::{HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation},
        System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
    };
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: both calls only read and flag a handle this process owns. A missing or invalid
        // handle is skipped, and a failure to flag one is harmless.
        unsafe {
            let handle = GetStdHandle(which);
            if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

/// Where the app is, and the token it wants.
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// `http://127.0.0.1:<port>`.
    pub base: String,
    /// The bearer token.
    pub token: String,
}

/// Finds the running app, and starts it when it is not running. The `verteller mcp` stdio bridge
/// runs as a process of its own before any server exists, so the app hands the Pro build this
/// instead of the data folder and a launcher: the Pro side never reads the data folder or starts
/// the app itself.
#[async_trait::async_trait]
pub trait EndpointLocator: Send + Sync {
    /// The app's address and token. When the app does not answer, starts it and waits for it, so
    /// this can take a while. The text of an error is the sentence for the person.
    async fn locate(&self) -> Result<Endpoint, String>;
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn the_app_breaks_away_from_a_job_and_starts_inside_one_that_refuses() {
        let mut tried = Vec::new();
        spawn_breaking_away(8, |flags| {
            tried.push(flags);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            tried,
            [8 | CREATE_BREAKAWAY_FROM_JOB],
            "outside a job, one try"
        );

        let mut tried = Vec::new();
        spawn_breaking_away(8, |flags| {
            tried.push(flags);
            if flags & CREATE_BREAKAWAY_FROM_JOB != 0 {
                Err(io::Error::from_raw_os_error(5))
            } else {
                Ok(())
            }
        })
        .unwrap();
        assert_eq!(tried, [8 | CREATE_BREAKAWAY_FROM_JOB, 8]);

        let mut tried = 0;
        let e = spawn_breaking_away(8, |_| {
            tried += 1;
            Err(io::Error::from_raw_os_error(2))
        })
        .unwrap_err();
        assert_eq!(
            (tried, e.raw_os_error()),
            (1, Some(2)),
            "other failures are not retried"
        );
    }
}
