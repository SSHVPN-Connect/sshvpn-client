use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes, GetSystemTimes};

static FILE_LOCK: Mutex<()> = Mutex::new(());
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
const SAMPLE_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy)]
pub(crate) struct Log(u64);

impl Log {
    pub(crate) fn new() -> Self {
        Self(NEXT_SESSION.fetch_add(1, Ordering::Relaxed))
    }

    fn write(self, action: &str, details: std::fmt::Arguments<'_>) {
        let Ok(directory) = crate::profiles::Profiles::directory() else {
            return;
        };
        let Ok(_lock) = FILE_LOCK.lock() else { return };
        if fs::create_dir_all(&directory).is_err() {
            return;
        }
        let path = directory.join("vpn-diagnostics.log");
        if fs::metadata(&path).is_ok_and(|meta| meta.len() >= 1024 * 1024) {
            let _ = fs::rename(&path, directory.join("vpn-diagnostics.previous.log"));
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();
            let _ = writeln!(
                file,
                "unix_ms={} pid={} session={} version={} action={action} {details}",
                timestamp.as_millis(),
                std::process::id(),
                self.0,
                env!("CARGO_PKG_VERSION")
            );
        }
    }

    pub(crate) fn step<T, E>(
        self,
        stage: &'static str,
        work: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let start = Instant::now();
        self.write("stage_begin", format_args!("stage={stage}"));
        let result = work();
        self.write(
            "stage_end",
            format_args!(
                "stage={stage} elapsed_ms={} ok={}",
                start.elapsed().as_millis(),
                result.is_ok()
            ),
        );
        result
    }

    pub(crate) fn begin(self, phase: &'static str) -> Trace {
        let start = Instant::now();
        self.write("begin", format_args!("phase={phase}"));
        let initial = CpuTimes::read();
        let (stop, rx) = mpsc::channel();
        let _ = thread::Builder::new().name("vpn-diagnostics".into()).spawn(move || {
            let mut previous = initial;
            loop {
                let finished = !matches!(rx.recv_timeout(SAMPLE_INTERVAL), Err(mpsc::RecvTimeoutError::Timeout));
                let current = CpuTimes::read();
                let cpu = previous.zip(current).and_then(|(before, after)| after.load_since(before));
                let mut memory = MEMORYSTATUSEX {
                    dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
                    ..Default::default()
                };
                let ram = (unsafe { GlobalMemoryStatusEx(&mut memory) } != 0)
                    .then_some(memory.dwMemoryLoad);
                self.write("sample", format_args!(
                    "phase={phase} elapsed_ms={} cpu_system_pct={:?} cpu_client_pct={:?} ram_system_pct={ram:?}",
                    start.elapsed().as_millis(), cpu.map(|load| load.0), cpu.map(|load| load.1)
                ));
                previous = current;
                if finished {
                    break;
                }
            }
        });
        Trace {
            log: self,
            phase,
            start,
            _stop: stop,
        }
    }
}

pub(crate) struct Trace {
    log: Log,
    phase: &'static str,
    start: Instant,
    _stop: mpsc::Sender<()>,
}

impl Trace {
    pub(crate) fn finish(self, outcome: &'static str) {
        self.log.write(
            "end",
            format_args!(
                "phase={} elapsed_ms={} outcome={outcome}",
                self.phase,
                self.start.elapsed().as_millis()
            ),
        );
    }
}

#[derive(Clone, Copy)]
struct CpuTimes {
    total: u64,
    idle: u64,
    client: u64,
}

impl CpuTimes {
    fn read() -> Option<Self> {
        fn ticks(time: FILETIME) -> u64 {
            (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
        }
        let [mut idle, mut kernel, mut user, mut created, mut exited, mut client_kernel, mut client_user] =
            [FILETIME::default(); 7];
        let system = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
        let client = unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut created,
                &mut exited,
                &mut client_kernel,
                &mut client_user,
            )
        };
        (system != 0 && client != 0).then(|| Self {
            total: ticks(kernel) + ticks(user),
            idle: ticks(idle),
            client: ticks(client_kernel) + ticks(client_user),
        })
    }

    fn load_since(self, before: Self) -> Option<(u64, u64)> {
        let total = self.total.checked_sub(before.total)?;
        if total == 0 {
            return None;
        }
        let idle = self.idle.checked_sub(before.idle)?;
        let client = self.client.checked_sub(before.client)?;
        Some((
            100 * total.saturating_sub(idle) / total,
            (100 * client / total).min(100),
        ))
    }
}
