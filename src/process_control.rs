// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use std::io;

#[cfg(unix)]
pub(crate) fn suspend_process(pid: u32) -> io::Result<()> {
    let rc = unsafe { libc::kill(pid as libc::pid_t, libc::SIGSTOP) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
pub(crate) fn resume_process(pid: u32) -> io::Result<()> {
    let rc = unsafe { libc::kill(pid as libc::pid_t, libc::SIGCONT) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(windows)]
pub(crate) fn suspend_process(pid: u32) -> io::Result<()> {
    use std::mem::size_of;
    use winapi::shared::minwindef::FALSE;
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::processthreadsapi::{OpenThread, ResumeThread, SuspendThread};
    use winapi::um::tlhelp32::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use winapi::um::winnt::THREAD_SUSPEND_RESUME;

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }

        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            cntUsage: 0,
            th32ThreadID: 0,
            th32OwnerProcessID: 0,
            tpBasePri: 0,
            tpDeltaPri: 0,
            dwFlags: 0,
        };

        let mut saw_thread = false;
        let mut first_err = None;
        let mut suspended_thread_ids = Vec::new();
        if Thread32First(snapshot, &mut entry) != FALSE {
            loop {
                if entry.th32OwnerProcessID == pid {
                    saw_thread = true;
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, FALSE, entry.th32ThreadID);
                    if thread.is_null() {
                        if first_err.is_none() {
                            first_err = Some(io::Error::last_os_error());
                        }
                    } else {
                        let result = SuspendThread(thread);
                        if result == u32::MAX && first_err.is_none() {
                            first_err = Some(io::Error::last_os_error());
                        }
                        if result != u32::MAX {
                            suspended_thread_ids.push(entry.th32ThreadID);
                        }
                        CloseHandle(thread);
                    }
                }
                if Thread32Next(snapshot, &mut entry) == FALSE {
                    break;
                }
            }
        } else {
            let err = io::Error::last_os_error();
            CloseHandle(snapshot);
            return Err(err);
        }
        CloseHandle(snapshot);

        if let Some(err) = first_err {
            for thread_id in suspended_thread_ids {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, FALSE, thread_id);
                if thread.is_null() {
                    continue;
                }
                let _ = ResumeThread(thread);
                CloseHandle(thread);
            }
            return Err(err);
        }
        if !saw_thread {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "process does not have any threads",
            ));
        }
        Ok(())
    }
}

#[cfg(windows)]
pub(crate) fn resume_process(pid: u32) -> io::Result<()> {
    use std::mem::size_of;
    use winapi::shared::minwindef::FALSE;
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::processthreadsapi::{OpenThread, ResumeThread};
    use winapi::um::tlhelp32::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use winapi::um::winnt::THREAD_SUSPEND_RESUME;

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }

        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            cntUsage: 0,
            th32ThreadID: 0,
            th32OwnerProcessID: 0,
            tpBasePri: 0,
            tpDeltaPri: 0,
            dwFlags: 0,
        };

        let mut saw_thread = false;
        let mut first_err = None;
        if Thread32First(snapshot, &mut entry) != FALSE {
            loop {
                if entry.th32OwnerProcessID == pid {
                    saw_thread = true;
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, FALSE, entry.th32ThreadID);
                    if thread.is_null() {
                        if first_err.is_none() {
                            first_err = Some(io::Error::last_os_error());
                        }
                    } else {
                        loop {
                            let result = ResumeThread(thread);
                            if result == u32::MAX {
                                if first_err.is_none() {
                                    first_err = Some(io::Error::last_os_error());
                                }
                                break;
                            }
                            if result <= 1 {
                                break;
                            }
                        }
                        CloseHandle(thread);
                    }
                }
                if Thread32Next(snapshot, &mut entry) == FALSE {
                    break;
                }
            }
        } else {
            let err = io::Error::last_os_error();
            CloseHandle(snapshot);
            return Err(err);
        }
        CloseHandle(snapshot);

        if let Some(err) = first_err {
            return Err(err);
        }
        if !saw_thread {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "process does not have any threads",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{resume_process, suspend_process};
    use std::fs;
    use std::path::PathBuf;
    use std::process::{Child, Command};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_suspend_and_resume_process() {
        suspend_and_resume_writer_process();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_suspend_and_resume_process() {
        suspend_and_resume_writer_process();
    }

    #[cfg(unix)]
    fn suspend_and_resume_writer_process() {
        let path = unique_temp_path("twatch-pause-unix");
        let mut child = spawn_file_writer(&path);
        wait_for_growth(&path, 0);
        let before = file_len(&path);

        suspend_process(child.id()).unwrap();
        thread::sleep(Duration::from_millis(400));
        let paused_len = file_len(&path);
        thread::sleep(Duration::from_millis(250));
        assert_eq!(paused_len, file_len(&path));

        resume_process(child.id()).unwrap();
        wait_for_growth(&path, paused_len);
        assert!(file_len(&path) >= before);

        cleanup(&mut child, &path);
    }

    #[cfg(windows)]
    #[test]
    fn windows_suspend_and_resume_process() {
        let path = unique_temp_path("twatch-pause-windows");
        let mut child = spawn_file_writer(&path);
        wait_for_growth(&path, 0);

        suspend_process(child.id()).unwrap();
        thread::sleep(Duration::from_millis(500));
        let paused_len = file_len(&path);
        thread::sleep(Duration::from_millis(300));
        assert_eq!(paused_len, file_len(&path));

        resume_process(child.id()).unwrap();
        wait_for_growth(&path, paused_len);

        cleanup(&mut child, &path);
    }

    #[cfg(windows)]
    #[test]
    fn windows_suspend_and_resume_process_is_idempotent_across_toggle_cycle() {
        let path = unique_temp_path("twatch-pause-windows");
        let mut child = spawn_file_writer(&path);
        wait_for_growth(&path, 0);

        suspend_process(child.id()).unwrap();
        thread::sleep(Duration::from_millis(500));
        let paused_len = file_len(&path);
        thread::sleep(Duration::from_millis(300));
        assert_eq!(paused_len, file_len(&path));

        resume_process(child.id()).unwrap();
        wait_for_growth(&path, paused_len);

        cleanup(&mut child, &path);
    }

    fn cleanup(child: &mut Child, path: &PathBuf) {
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_file(path);
    }

    #[cfg(unix)]
    fn spawn_file_writer(path: &PathBuf) -> Child {
        Command::new("sh")
            .arg("-c")
            .arg(format!(
                "while :; do printf . >> '{}'; sleep 0.1; done",
                path.display()
            ))
            .spawn()
            .unwrap()
    }

    #[cfg(windows)]
    fn spawn_file_writer(path: &PathBuf) -> Child {
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "while ($true) {{ Add-Content -NoNewline -Path '{}' -Value '.'; Start-Sleep -Milliseconds 100 }}",
                    path.display()
                ),
            ])
            .spawn()
            .unwrap()
    }

    fn wait_for_growth(path: &PathBuf, previous_len: u64) {
        for _ in 0..30 {
            if file_len(path) > previous_len {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
        panic!("writer did not grow file");
    }

    fn file_len(path: &PathBuf) -> u64 {
        fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0)
    }

    fn unique_temp_path(prefix: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{suffix}.txt"))
    }
}
