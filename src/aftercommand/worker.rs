use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};

use crate::aftercommand::AfterCommandPayload;
use crate::cli::default_shell;

pub(super) fn run_hook_with_timeout(
    shell_program: &str,
    shell_args: &[String],
    hook: &str,
    timeout: Duration,
    payload: &AfterCommandPayload,
) -> Result<()> {
    let mut cmd = Command::new(shell_program);
    cmd.args(shell_args)
        .arg(hook)
        .env(
            "TWATCH_DATA",
            serde_json::to_string(payload).context("failed to serialize aftercommand payload")?,
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut child = spawn_hook_process(&mut cmd).context("failed to spawn aftercommand hook")?;
    let start = SystemTime::now();

    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if start.elapsed().unwrap_or(Duration::ZERO) >= timeout {
            let _ = child.kill_tree();
            let _ = child.wait();
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn parse_shell(shell: &str) -> (String, Vec<String>) {
    match shell_words::split(shell) {
        Ok(parts) if !parts.is_empty() => {
            (parts[0].clone(), parts.iter().skip(1).cloned().collect())
        }
        _ => {
            let parts = shell_words::split(&default_shell()).expect("default shell must parse");
            (parts[0].clone(), parts.iter().skip(1).cloned().collect())
        }
    }
}

struct HookProcess {
    child: std::process::Child,
    #[cfg(unix)]
    process_group_id: u32,
    #[cfg(windows)]
    job_handle: winapi::shared::ntdef::HANDLE,
}

impl HookProcess {
    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait()
    }

    fn kill_tree(&mut self) -> std::io::Result<()> {
        kill_hook_process_tree(self)
    }
}

#[cfg(unix)]
fn spawn_hook_process(cmd: &mut Command) -> std::io::Result<HookProcess> {
    use std::os::unix::process::CommandExt;

    unsafe {
        cmd.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }

    let child = cmd.spawn()?;
    let process_group_id = child.id();
    Ok(HookProcess {
        child,
        process_group_id,
    })
}

#[cfg(unix)]
fn kill_hook_process_tree(process: &mut HookProcess) -> std::io::Result<()> {
    let rc = unsafe { libc::kill(-(process.process_group_id as libc::pid_t), libc::SIGKILL) };
    if rc == 0 {
        Ok(())
    } else {
        let err = std::io::Error::last_os_error();
        if matches!(err.raw_os_error(), Some(libc::ESRCH)) {
            Ok(())
        } else {
            Err(err)
        }
    }
}

#[cfg(windows)]
fn spawn_hook_process(cmd: &mut Command) -> std::io::Result<HookProcess> {
    use winapi::shared::minwindef::FALSE;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::jobapi2::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
    };
    use winapi::um::processthreadsapi::OpenProcess;
    use winapi::um::winnt::{
        HANDLE, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, PROCESS_QUERY_INFORMATION, PROCESS_SET_QUOTA,
        PROCESS_TERMINATE,
    };

    let mut child = cmd.spawn()?;

    unsafe {
        let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
        if job.is_null() {
            let err = std::io::Error::last_os_error();
            let _ = child.kill();
            let _ = child.wait();
            return Err(err);
        }

        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &mut info as *mut _ as *mut _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if ok == FALSE {
            let err = std::io::Error::last_os_error();
            CloseHandle(job);
            let _ = child.kill();
            let _ = child.wait();
            return Err(err);
        }

        let process: HANDLE = OpenProcess(
            PROCESS_SET_QUOTA | PROCESS_TERMINATE | PROCESS_QUERY_INFORMATION,
            FALSE,
            child.id(),
        );
        if process.is_null() {
            let err = std::io::Error::last_os_error();
            CloseHandle(job);
            let _ = child.kill();
            let _ = child.wait();
            return Err(err);
        }

        let assigned = AssignProcessToJobObject(job, process);
        CloseHandle(process);
        if assigned == FALSE {
            let err = std::io::Error::last_os_error();
            CloseHandle(job);
            let _ = child.kill();
            let _ = child.wait();
            return Err(err);
        }

        Ok(HookProcess {
            child,
            job_handle: job,
        })
    }
}

#[cfg(windows)]
fn kill_hook_process_tree(process: &mut HookProcess) -> std::io::Result<()> {
    use winapi::shared::minwindef::FALSE;
    use winapi::um::jobapi2::TerminateJobObject;

    unsafe {
        if TerminateJobObject(process.job_handle, 1) == FALSE {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(windows)]
impl Drop for HookProcess {
    fn drop(&mut self) {
        unsafe {
            if !self.job_handle.is_null() {
                winapi::um::handleapi::CloseHandle(self.job_handle);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::run_hook_with_timeout;
    use crate::aftercommand::AfterCommandPayload;
    use std::fs;
    use std::time::Duration;

    #[cfg(unix)]
    #[test]
    fn timeout_kills_hook_process_group() {
        let path = std::env::temp_dir().join(format!(
            "twatch-aftercommand-timeout-{}.txt",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);

        let payload = AfterCommandPayload {
            command: "demo".to_string(),
            changed: true,
            output: "output".to_string(),
            unix_timestamp: 1,
            frame_seq: 1,
            width: 80,
            height: 24,
            changed_cell_count: 1,
            matched_rules: vec!["changed".to_string()],
            last_input_summary: String::new(),
        };

        let hook = format!("(sleep 0.4; echo leaked > {}) & wait", path.display());
        run_hook_with_timeout(
            "sh",
            &["-c".to_string()],
            &hook,
            Duration::from_millis(100),
            &payload,
        )
        .unwrap();

        std::thread::sleep(Duration::from_millis(700));
        assert!(!path.exists(), "hook descendant survived timeout");
    }
}
