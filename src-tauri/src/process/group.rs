//! Platform-specific "kill the whole tree" support.

#[cfg(windows)]
pub use windows_impl::{configure, KillGroup};

#[cfg(unix)]
pub use unix_impl::{configure, KillGroup};

#[cfg(windows)]
mod windows_impl {
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::ptr::null;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    use crate::error::{AppError, AppResult};

    /// Suppress the console window a GUI app would otherwise flash for each child.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn configure(cmd: &mut tokio::process::Command) {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    /// A Job Object configured with KILL_ON_JOB_CLOSE: if TaskKiln exits or
    /// crashes, Windows closes the handle and terminates every process in it.
    pub struct KillGroup {
        job: HANDLE,
    }

    // SAFETY: a job handle is a kernel object handle usable from any thread.
    unsafe impl Send for KillGroup {}
    unsafe impl Sync for KillGroup {}

    impl KillGroup {
        pub fn attach(child: &tokio::process::Child) -> AppResult<Self> {
            let process = child
                .raw_handle()
                .ok_or_else(|| AppError::Process("child exited before it could be tracked".into()))?;
            // SAFETY: plain Win32 calls with valid arguments; the handle is owned by `Self`.
            unsafe {
                let job = CreateJobObjectW(null(), null());
                if job.is_null() {
                    return Err(AppError::Process("CreateJobObjectW failed".into()));
                }
                let group = Self { job };
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const c_void,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 || AssignProcessToJobObject(job, process as HANDLE) == 0 {
                    return Err(AppError::Process("could not attach process to job object".into()));
                }
                Ok(group)
            }
        }

        pub fn kill(&self) {
            // SAFETY: `job` is a valid handle for the lifetime of `self`.
            unsafe {
                TerminateJobObject(self.job, 1);
            }
        }
    }

    impl Drop for KillGroup {
        fn drop(&mut self) {
            // SAFETY: closing our own handle; KILL_ON_JOB_CLOSE reaps leftovers.
            unsafe {
                CloseHandle(self.job);
            }
        }
    }
}

#[cfg(unix)]
mod unix_impl {
    use std::time::Duration;

    use crate::error::{AppError, AppResult};

    const TERM_GRACE: Duration = Duration::from_secs(3);

    /// Put the child in its own process group so the whole tree can be signalled.
    pub fn configure(cmd: &mut tokio::process::Command) {
        cmd.process_group(0);
    }

    pub struct KillGroup {
        pgid: i32,
    }

    impl KillGroup {
        pub fn attach(child: &tokio::process::Child) -> AppResult<Self> {
            let pid = child
                .id()
                .ok_or_else(|| AppError::Process("child exited before it could be tracked".into()))?;
            Ok(Self { pgid: pid as i32 })
        }

        /// SIGTERM the group, then SIGKILL whatever is left after a grace period.
        pub fn kill(&self) {
            let pgid = self.pgid;
            // SAFETY: signalling a process group we created; ESRCH is harmless.
            unsafe {
                libc::kill(-pgid, libc::SIGTERM);
            }
            std::thread::spawn(move || {
                std::thread::sleep(TERM_GRACE);
                unsafe {
                    libc::kill(-pgid, libc::SIGKILL);
                }
            });
        }
    }
}
