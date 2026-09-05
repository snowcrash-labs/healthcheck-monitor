//! Bounded process-tree sampling; no command lines, environments or payloads are inspected.
use std::collections::BTreeSet;

#[derive(Default)]
pub struct Sample {
    pub rss: u64,
    pub root_rss: u64,
    pub processes: usize,
    pub pids: BTreeSet<i32>,
    pub incomplete: bool,
}

pub fn tree(root: i32) -> Sample {
    let mut sample = Sample::default();
    let mut pending = vec![root];
    while let Some(pid) = pending.pop() {
        if sample.pids.contains(&pid) {
            continue;
        }
        if sample.pids.len() >= 512 || pending.len() >= 512 {
            sample.incomplete = true;
            break;
        }
        sample.pids.insert(pid);
        if let Some(rss) = rss(pid) {
            sample.rss += rss;
            sample.processes += 1;
            if pid == root {
                sample.root_rss = rss;
            }
        }
        match children(pid) {
            Some(children) => pending.extend(children),
            None => sample.incomplete = true,
        }
    }
    sample
}

#[cfg(target_os = "macos")]
fn rss(pid: i32) -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_taskinfo>() as i32;
    // The fixed ABI buffer is read only after the kernel reports a complete structure.
    let count = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    (count == size).then(|| unsafe { info.assume_init().pti_resident_size })
}

#[cfg(target_os = "macos")]
fn children(pid: i32) -> Option<Vec<i32>> {
    let mut children = [0i32; 512];
    let bytes = std::mem::size_of_val(&children) as i32;
    // This wrapper converts the underlying byte count into a PID count.
    let count = unsafe { libc::proc_listchildpids(pid, children.as_mut_ptr().cast(), bytes) };
    if count < 0 || count as usize >= children.len() {
        return None;
    }
    Some(
        children[..count as usize]
            .iter()
            .copied()
            .filter(|pid| *pid > 0)
            .collect(),
    )
}

#[cfg(target_os = "linux")]
fn rss(pid: i32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix("VmRSS:")?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()
            .map(|kib| kib * 1024)
    })
}

#[cfg(target_os = "linux")]
fn children(pid: i32) -> Option<Vec<i32>> {
    let tasks = std::fs::read_dir(format!("/proc/{pid}/task")).ok()?;
    let mut children = BTreeSet::new();
    // A child belongs to the thread that spawned it, not necessarily the group leader.
    for (index, task) in tasks.enumerate() {
        if index >= 512 {
            return None;
        }
        let Ok(task) = task else { continue };
        let Ok(value) = std::fs::read_to_string(task.path().join("children")) else {
            continue;
        };
        if value.len() > 8192 {
            return None;
        }
        for child in value
            .split_whitespace()
            .filter_map(|pid| pid.parse::<i32>().ok())
        {
            if children.len() >= 512 {
                return None;
            }
            children.insert(child);
        }
    }
    Some(children.into_iter().collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn descendants_are_included_in_the_memory_sample() -> Result<(), Box<dyn std::error::Error>> {
        let mut child = std::process::Command::new("/bin/sleep").arg("1").spawn()?;
        std::thread::sleep(std::time::Duration::from_millis(30));
        let sample = super::tree(i32::try_from(std::process::id())?);
        let child_pid = i32::try_from(child.id())?;
        child.kill()?;
        child.wait()?;
        assert!(sample.pids.contains(&child_pid));
        assert!(sample.rss > sample.root_rss);
        Ok(())
    }

    #[test]
    fn running_process_has_resident_memory() -> Result<(), Box<dyn std::error::Error>> {
        let pid = i32::try_from(std::process::id())?;
        let sample = super::tree(pid);
        assert!(sample.root_rss > 0);
        assert!(sample.rss >= sample.root_rss);
        assert!(sample.pids.contains(&pid));
        Ok(())
    }
}
