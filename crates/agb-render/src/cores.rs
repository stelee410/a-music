//! Keeping the render on the fast cores of a chip that has slow ones too (phones, handhelds).

/// The cores of the machine that reach the highest clock rate (the "big" ones of a phone chip).
#[cfg(any(target_os = "android", target_os = "linux"))]
fn fastest_cores() -> Vec<usize> {
    let rate = |cpu: usize| std::fs::read_to_string(format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/cpuinfo_max_freq")).ok().and_then(|s| s.trim().parse::<u64>().ok());
    let rates: Vec<(usize, u64)> = (0..64).map_while(|cpu| rate(cpu).map(|r| (cpu, r))).collect();
    let top = rates.iter().map(|r| r.1).max().unwrap_or(0);
    rates.into_iter().filter(|r| r.1 == top).map(|r| r.0).collect()
}

/// Keep the calling thread, and the threads it starts from now on (the singers), on the fastest
/// cores. A thread that has just been started is otherwise put on a slow core first, where a
/// simulated voice takes three times as long. Returns how many cores that is, or 0 where it was
/// left as it was (other systems, or a machine whose cores are all alike and cannot be read).
pub fn use_fast_cores() -> usize {
    #[cfg(any(target_os = "android", target_os = "linux"))]
    {
        let cores = fastest_cores();
        if cores.is_empty() {
            return 0;
        }
        // SAFETY: a zeroed cpu_set_t is an empty set; the calls only read and write that set.
        unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            for c in &cores {
                libc::CPU_SET(*c, &mut set);
            }
            if libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set) != 0 {
                return 0;
            }
        }
        cores.len()
    }
    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    {
        0
    }
}
