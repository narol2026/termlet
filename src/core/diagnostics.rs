// Diagnostics subsystem
//
// The whole machinery (log writer, freeze watchdog, SIGUSR1 stack sampler,
// performance timing) is compiled into both debug and release builds. What it
// does at runtime is controlled by `configure(enabled, level, path)`: when
// disabled, or when a call site's level is above the configured level, every
// entry point takes a cheap fast path and does nothing.
//
// Defaults (before/without `configure`):
//   enabled = true
//   level   = Trace in debug builds, Warn in release builds
//   path    = <config dir>/terminal_diagnostics.log
//
// `Warn` — the release default — captures freeze and failure lines only, so it
// never writes host identities, filesystem paths, forwarded env vars, or
// per-frame timings; those live at `Info`/`Debug`/`Trace` and are opt-in.
mod imp {

    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::{Path, PathBuf};
    #[cfg(unix)]
    use std::sync::Mutex;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
    #[cfg(unix)]
    use std::sync::atomic::{AtomicU32, AtomicUsize};
    use std::sync::mpsc::{self, SyncSender, TrySendError};
    use std::thread::ThreadId;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use crate::core::paths;
    use crate::core::time::format_log_time;

    const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;

    // Diagnostic level / configuration
    // A call site's level is the *first* argument to `log`/`perf_guard`; a line
    // is written only when `site_level <= configured`.

    #[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    #[repr(u8)]
    pub enum Level {
        Error = 0,
        Warn = 1,
        Info = 2,
        Debug = 3,
        Trace = 4,
    }

    impl Level {
        /// Parse the persisted preference string; `None` when unknown.
        pub fn parse(s: &str) -> Option<Level> {
            match s {
                "error" => Some(Level::Error),
                "warn" => Some(Level::Warn),
                "info" => Some(Level::Info),
                "debug" => Some(Level::Debug),
                "trace" => Some(Level::Trace),
                _ => None,
            }
        }

        pub fn as_str(self) -> &'static str {
            match self {
                Level::Error => "error",
                Level::Warn => "warn",
                Level::Info => "info",
                Level::Debug => "debug",
                Level::Trace => "trace",
            }
        }
    }

    // Verbose in dev, quiet (freeze/error only) in release.
    const DEFAULT_LEVEL_U8: u8 = if cfg!(debug_assertions) {
        4 /* Trace */
    } else {
        1 /* Warn */
    };

    static ENABLED: AtomicBool = AtomicBool::new(true);
    static LEVEL: AtomicU8 = AtomicU8::new(DEFAULT_LEVEL_U8);
    static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
    static LOG_PERMISSION_FIXED: OnceLock<PathBuf> = OnceLock::new();

    fn level() -> Level {
        match LEVEL.load(Ordering::Relaxed) {
            0 => Level::Error,
            1 => Level::Warn,
            2 => Level::Info,
            3 => Level::Debug,
            _ => Level::Trace,
        }
    }

    /// Whether a call site at `site_level` should do its work, given the
    /// current enabled flag and configured level.
    pub fn enabled_at(site_level: Level) -> bool {
        ENABLED.load(Ordering::Relaxed) && site_level as u8 <= LEVEL.load(Ordering::Relaxed)
    }

    /// Configure the subsystem.  `path` overrides the log file location.
    /// When the configured level enables stack sampling (Debug or lower),
    /// the SIGUSR1 handler is installed here — this must run on the main
    /// thread, which is the case when called from `App::new`.
    pub fn configure(enabled: bool, level: Level, path: Option<PathBuf>) {
        ENABLED.store(enabled, Ordering::Relaxed);
        LEVEL.store(level as u8, Ordering::Relaxed);
        if let Some(p) = path {
            let _ = LOG_PATH.set(p);
        }
        #[cfg(unix)]
        if enabled_at(Level::Debug) {
            stack_sampler::init();
        }
    }

    pub fn default_level() -> Level {
        level()
    }

    /// Change only the verbosity level at runtime (the settings surface).
    /// Leaves the enabled flag and log path untouched.  Installs the
    /// SIGUSR1 stack-sampler handler when the new level first enables it.
    pub fn set_level(level: Level) {
        LEVEL.store(level as u8, Ordering::Relaxed);
        #[cfg(unix)]
        if enabled_at(Level::Debug) {
            stack_sampler::init();
        }
    }

    // Performance heartbeat counter
    // Incremented on every update() and view() call, so the log correlates slow
    // operations with the event loop cadence.
    static FRAME_COUNTER: AtomicU64 = AtomicU64::new(0);

    pub fn next_frame_id() -> u64 {
        if !enabled_at(Level::Debug) {
            return 0;
        }
        FRAME_COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    // Freeze Watchdog
    // A background thread watches the timestamp `watchdog_ping()` stores on
    // every update() call. When the main thread blocks — synchronous I/O, a long
    // computation, a GPU driver stall — update() stops running, the thread sees
    // the gap, and it asks the frozen thread to snapshot its own stack so the
    // log shows *where* it is stuck rather than only that it happened. On Linux
    // the scheduler state comes from /proc, which needs no signal handler. The
    // thread starts on the first ping, so an idle AnimTick costs nothing.

    const FREEZE_GAP_MS: u64 = 1000; // gap longer than this ⇒ freeze
    const STACK_SAMPLE_INTERVAL_MS: u64 = 1500; // between samples while frozen
    const MAX_SAMPLES_PER_FREEZE: u32 = 10; // cap per freeze episode

    static WATCHDOG_ENABLED: AtomicBool = AtomicBool::new(false);
    static LAST_PING_MS: AtomicU64 = AtomicU64::new(0);

    /// Called from the main thread (`update()`) to signal that the event
    /// loop is alive.  The first call spawns the watchdog thread.
    pub fn watchdog_ping() {
        if !enabled_at(Level::Warn) {
            return;
        }

        LAST_PING_MS.store(now_ms(), Ordering::Relaxed);

        // Lazy-init: spawn the watchdog thread on first ping.
        if !WATCHDOG_ENABLED.swap(true, Ordering::Relaxed) {
            std::thread::Builder::new()
                .name("termlet-freeze-watchdog".into())
                .spawn(watchdog_loop)
                .expect("failed to spawn freeze watchdog thread");
        }
    }

    fn watchdog_loop() {
        let mut frozen = false;
        #[cfg(unix)]
        let mut episode = 0u32;
        let mut frozen_since_ms = 0u64;
        let mut last_sample_ms = 0u64;
        let mut samples_taken = 0u32;

        while WATCHDOG_ENABLED.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(500));

            if !enabled_at(Level::Warn) {
                frozen = false;
                continue;
            }

            let last_ms = LAST_PING_MS.load(Ordering::Relaxed);
            if last_ms == 0 {
                continue; // not yet initialised
            }

            let now_ms = now_ms();
            let gap_ms = now_ms.saturating_sub(last_ms);

            if gap_ms > FREEZE_GAP_MS {
                if !frozen {
                    frozen = true;
                    #[cfg(unix)]
                    {
                        episode += 1;
                    };
                    frozen_since_ms = now_ms;
                    last_sample_ms = 0;
                    samples_taken = 0;
                    log(
                        Level::Warn,
                        "FREEZE",
                        format!(
                            "UI freeze detected — no main-thread activity for {gap_ms} ms \
                         (≈{:.1}s).  The event loop is blocked.",
                            gap_ms as f64 / 1000.0
                        ),
                    );
                    log_kernel_state();
                }

                // While still frozen, snapshot the main-thread stack a few
                // times so the log shows what it is stuck in.  Gated on
                // Debug so the Warn default never sends SIGUSR1.
                if enabled_at(Level::Debug)
                    && now_ms.saturating_sub(last_sample_ms) >= STACK_SAMPLE_INTERVAL_MS
                    && samples_taken < MAX_SAMPLES_PER_FREEZE
                {
                    last_sample_ms = now_ms;
                    samples_taken += 1;
                    #[cfg(unix)]
                    stack_sampler::sample_main_thread(episode, samples_taken);
                }
            } else if frozen {
                let dur_ms = now_ms.saturating_sub(frozen_since_ms);
                log(
                    Level::Warn,
                    "FREEZE",
                    format!(
                        "UI recovered after {dur_ms} ms (≈{:.1}s), {samples_taken} stack sample(s) taken.",
                        dur_ms as f64 / 1000.0
                    ),
                );
                frozen = false;
            }
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    /// Best-effort Linux-only kernel insight: the wait channel the main
    /// thread is blocked in and its scheduler state. /proc is read from
    /// the watchdog thread, so this carries none of the signal-handler
    /// risk. wchan may be empty or require privileges — best effort.
    #[cfg(target_os = "linux")]
    fn log_kernel_state() {
        let tid = stack_sampler::main_pthread();
        if tid == 0 {
            return;
        }
        let pid = std::process::id();
        if let Ok(wchan) = fs::read_to_string(format!("/proc/{pid}/task/{tid}/wchan")) {
            let wchan = wchan.trim();
            if !wchan.is_empty() {
                log(
                    Level::Warn,
                    "FREEZE",
                    format!("main thread kernel wait channel: {wchan}"),
                );
            }
        }
        // stat field 3: R = running, S = sleeping, D = uninterruptible.
        if let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/task/{tid}/stat"))
            && let Some(state) = stat.split_whitespace().nth(2)
        {
            log(
                Level::Warn,
                "FREEZE",
                format!("main thread scheduler state: {state}"),
            );
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn log_kernel_state() {}

    // Main-thread stack sampler
    // Rust stacks are private to their thread, so the watchdog cannot read the
    // main thread's from outside; it asks the thread to capture its own:
    //
    //   1. the watchdog signals the main thread with SIGUSR1.
    //   2. handler runs on the main thread, interrupting whatever it
    //      was stuck in, and records raw instruction pointers only
    //      (no allocation, no symbol resolution — signal-safe enough).
    //   3. the raw sample lands in a small ring; the watchdog drains it
    //      and resolves symbols in normal thread context (safe to
    //      allocate / take libc locks there).
    //
    // SA_RESTART is set so interrupting a blocking syscall just resumes
    // it — sampling never changes program behaviour. The ring is touched
    // only by the handler and the watchdog, never the main thread, so
    // taking the ring lock inside the handler can't deadlock against a
    // frozen main thread.

    #[cfg(unix)]
    mod stack_sampler {
        use super::{AtomicBool, AtomicU32, AtomicUsize, Duration, Level, Mutex, Ordering, log};

        const MAX_IPS: usize = 128;
        const RING_SIZE: usize = 4;
        const SAMPLE_SIGNAL: i32 = libc::SIGUSR1;

        #[derive(Copy, Clone)]
        struct Sample {
            seq: u32,
            count: usize,
            ips: [usize; MAX_IPS],
        }

        static RING: Mutex<[Option<Sample>; RING_SIZE]> = Mutex::new([None; RING_SIZE]);
        static SAMPLE_BUSY: AtomicBool = AtomicBool::new(false);
        static SAMPLE_SEQ: AtomicU32 = AtomicU32::new(0);
        static MAIN_PTHREAD: AtomicUsize = AtomicUsize::new(0);

        pub fn main_pthread() -> usize {
            MAIN_PTHREAD.load(Ordering::Relaxed)
        }

        /// Whether the calling thread is the process's main thread.
        #[cfg(target_os = "linux")]
        fn is_main_thread() -> bool {
            // SAFETY: both calls take no arguments and cannot fail; the calling
            // thread is the process's main thread exactly when its thread id and
            // the process id agree.
            unsafe { libc::gettid() == libc::getpid() }
        }

        /// Whether the calling thread is the process's main thread.
        #[cfg(target_os = "macos")]
        fn is_main_thread() -> bool {
            // SAFETY: `pthread_main_np` takes no arguments and cannot fail; it
            // reports whether the calling thread is the main one.
            unsafe { libc::pthread_main_np() != 0 }
        }

        /// Whether the calling thread is the process's main thread.
        ///
        /// No cheap check exists here, so the sampler keeps its previous
        /// behaviour rather than switching itself off.
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        fn is_main_thread() -> bool {
            true
        }

        /// Called once from `main()` (on the main thread): remember the main
        /// thread id and install the SIGUSR1 handler.
        pub fn init() {
            // The thread that calls this becomes the sampling target, so it has
            // to be the one that lives as long as the process. `pthread_kill` on
            // an id whose thread has since exited reads freed thread state and
            // takes the process down with it, and a test harness runs every test
            // on its own short-lived thread — so anything that is not the main
            // thread leaves the sampler off rather than arming it on an id that
            // will be recycled.
            if !is_main_thread() {
                return;
            }
            // SAFETY: `pthread_self` takes no arguments and returns the calling
            // thread's id, which is always valid on the thread it is called
            // from. `init` runs on the main thread, and that id is what
            // `sample_main_thread` later signals.
            MAIN_PTHREAD.store(unsafe { libc::pthread_self() } as usize, Ordering::Relaxed);
            // SAFETY: `act` is a `#[repr(C)]` struct of integers, so zeroing it
            // is a valid starting point, and every field `sigaction` reads
            // (`sa_sigaction`, `sa_mask`, `sa_flags`) is initialised next. The
            // handler has the `extern "C" fn(c_int)` signature the flag-free
            // form requires, and a null old-action pointer is accepted.
            unsafe {
                let mut act: libc::sigaction = std::mem::zeroed();
                // The handler is installed by address, never invoked from
                // here, so the `*const ()` hop clippy suggests removing is
                // what makes this a signal-handler installation.
                #[expect(
                    clippy::fn_to_numeric_cast_any,
                    reason = "the handler address is stored, not called"
                )]
                let handler = on_sample_signal as *const () as usize;
                act.sa_sigaction = handler;
                libc::sigemptyset(&raw mut act.sa_mask);
                act.sa_flags = libc::SA_RESTART;
                libc::sigaction(SAMPLE_SIGNAL, &raw const act, std::ptr::null_mut());
            }
        }

        /// Signal the main thread to capture its stack, then resolve and
        /// log it from this (safe) thread context.
        pub fn sample_main_thread(episode: u32, sample_no: u32) {
            let tid = main_pthread();
            if tid == 0 {
                return;
            }
            // SAFETY: `tid` is the `pthread_t` the main thread recorded in
            // `init` (zero, meaning "not recorded yet", is filtered out above),
            // and `SAMPLE_SIGNAL` is the signal that handler was installed for.
            // If the thread has already exited `pthread_kill` returns `ESRCH`
            // without touching anything, which is why the result is ignored.
            unsafe { libc::pthread_kill(tid as libc::pthread_t, SAMPLE_SIGNAL) };

            // The handler may run on the next instruction or be deferred if
            // the main thread is deep in a syscall — retry a few times.
            for _ in 0..5 {
                std::thread::sleep(Duration::from_millis(20));
                if drain(episode, sample_no) {
                    return;
                }
            }
        }

        extern "C" fn on_sample_signal(_sig: i32) {
            // Re-entrancy guard: a signal arriving mid-sample is dropped.
            if SAMPLE_BUSY.swap(true, Ordering::SeqCst) {
                return;
            }

            let mut sample = Sample {
                seq: SAMPLE_SEQ.fetch_add(1, Ordering::Relaxed),
                count: 0,
                ips: [0; MAX_IPS],
            };

            // Capture raw instruction pointers only — no resolution, no
            // allocation, keeps the handler cheap and re-entrancy-safe.
            let mut count = 0usize;
            backtrace::trace(|frame| {
                if count < MAX_IPS {
                    sample.ips[count] = frame.ip() as usize;
                    count += 1;
                }
                count < MAX_IPS
            });
            sample.count = count;

            if count > 0 {
                let mut ring = RING
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                for slot in ring.iter_mut() {
                    if slot.is_none() {
                        *slot = Some(sample);
                        break;
                    }
                }
            }

            SAMPLE_BUSY.store(false, Ordering::SeqCst);
        }

        /// Take pending samples off the ring and log them with symbols.
        /// Returns true when at least one sample was logged.
        fn drain(episode: u32, sample_no: u32) -> bool {
            let mut ring = RING
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let pending: Vec<Sample> = ring
                .iter_mut()
                .filter_map(std::option::Option::take)
                .collect();
            drop(ring);

            if pending.is_empty() {
                return false;
            }

            for sample in pending {
                log(
                    Level::Debug,
                    "STACK",
                    format!(
                        "episode #{episode}, sample #{sample_no} (seq {}): {} frames",
                        sample.seq, sample.count
                    ),
                );
                for (i, ip) in sample.ips[..sample.count].iter().enumerate() {
                    let addr = *ip as *mut libc::c_void;
                    let mut rendered = None;
                    backtrace::resolve(addr, |sym| {
                        let name = sym
                            .name()
                            .map_or_else(|| "<unknown>".to_string(), |n| n.to_string());
                        let file = sym
                            .filename()
                            .and_then(|f| f.file_name())
                            .and_then(|f| f.to_str())
                            .unwrap_or("?");
                        let line = sym.lineno().unwrap_or(0);
                        rendered = Some(format!("{name}  ({file}:{line})"));
                    });
                    match rendered {
                        Some(text) => log(Level::Debug, "STACK", format!("#{i:02}  {text}")),
                        None => log(
                            Level::Debug,
                            "STACK",
                            format!("#{i:02}  0x{:016x}", addr as usize),
                        ),
                    }
                }
            }
            true
        }
    }

    // AnimTick gap detection
    // Called from AnimTick on the main thread: records the timestamp and returns
    // the gap since the previous tick when it exceeds 500 ms, catching freezes
    // *after* they end, where the watchdog catches them *during*.

    static LAST_TICK_MS: AtomicU64 = AtomicU64::new(0);

    /// Returns Some(gap) when the interval since the previous call
    /// exceeds 500 ms, indicating the main thread was blocked.
    pub fn check_tick_gap() -> Option<Duration> {
        if !enabled_at(Level::Warn) {
            return None;
        }

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let last_ms = LAST_TICK_MS.swap(now_ms, Ordering::Relaxed);

        if last_ms == 0 {
            return None; // first tick after startup
        }

        let gap_ms = now_ms.saturating_sub(last_ms);
        (gap_ms > 500).then(|| Duration::from_millis(gap_ms))
    }

    // Background log writer
    // A dedicated thread writes the file, so a flood of log lines (every
    // terminal BackendCall during heavy output) can never stall the UI event
    // loop on file I/O. `log()` only formats the line and pushes it into a
    // bounded channel;
    // the writer thread drains the channel and appends to the file.  When
    // the queue is full the line is dropped (counted) rather than blocking
    // the caller, so the main thread stays responsive under sustained load.

    const LOG_QUEUE_CAPACITY: usize = 8192;

    struct LogLine {
        timestamp: String,
        scope: String,
        thread_id: ThreadId,
        message: String,
    }

    static LOG_SENDER: OnceLock<SyncSender<LogLine>> = OnceLock::new();
    static DROPPED_LOG_LINES: AtomicU64 = AtomicU64::new(0);

    fn log_sender() -> &'static SyncSender<LogLine> {
        LOG_SENDER.get_or_init(|| {
            let (tx, rx) = mpsc::sync_channel(LOG_QUEUE_CAPACITY);
            let spawn = std::thread::Builder::new()
                .name("termlet-log-writer".into())
                .spawn(move || log_writer_loop(&rx));
            if spawn.is_ok() {
                tx
            } else {
                let (tx, _rx) = mpsc::sync_channel(0);
                tx
            }
        })
    }

    fn log_writer_loop(rx: &mpsc::Receiver<LogLine>) {
        while let Ok(line) = rx.recv() {
            write_line(&line);
        }
    }

    fn write_line(line: &LogLine) {
        let path = log_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        ensure_log_file_private(&path);
        rotate_if_needed(&path);
        if let Ok(mut file) = open_log_append(&path) {
            let _ = writeln!(
                file,
                "[{}][{}][{:?}] {}",
                line.timestamp, line.scope, line.thread_id, line.message
            );
        }
    }

    fn open_log_append(path: &Path) -> std::io::Result<fs::File> {
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600)
        };
        options.open(path)
    }

    fn ensure_log_file_private(path: &PathBuf) {
        if LOG_PERMISSION_FIXED.get() == Some(path) {
            return;
        }
        if crate::core::paths::restrict_file_permissions(path).is_ok() {
            let _ = LOG_PERMISSION_FIXED.set(path.clone());
        }
    }

    pub fn log(level: Level, scope: &str, message: impl AsRef<str>) {
        if !enabled_at(level) {
            return;
        }
        let line = LogLine {
            timestamp: format_log_time(SystemTime::now()),
            scope: scope.to_owned(),
            thread_id: std::thread::current().id(),
            message: message.as_ref().to_owned(),
        };
        match log_sender().try_send(line) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                // Queue backed up under sustained load: drop rather than stall.
                DROPPED_LOG_LINES.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(_)) => {
                // Writer thread gone (shutdown) — drop silently.
            }
        }
    }

    // Performance timing
    // Two constructors keep hot paths allocation-free: `perf_guard` takes a
    // `&'static str` label and is the default, while `perf_guard_dyn` takes an
    // already-built `String` for the cases where naming the operation costs
    // nothing next to the I/O being measured. A disabled guard never even calls
    // `Instant::now()`.

    pub struct TimingGuard {
        active: bool,
        scope: &'static str,
        label: &'static str,
        dynamic_label: Option<String>,
        threshold: Duration,
        start: Option<Instant>,
        frame_id: u64,
    }

    impl TimingGuard {
        fn label_str(&self) -> &str {
            self.dynamic_label.as_deref().unwrap_or(self.label)
        }

        fn finish(&mut self) -> bool {
            let Some(start) = self.start else {
                return false;
            };
            let elapsed = start.elapsed();
            if elapsed >= self.threshold {
                log(
                    Level::Debug,
                    self.scope,
                    format!(
                        "[frame={}] {} took {:?} (threshold {:?})",
                        self.frame_id,
                        self.label_str(),
                        elapsed,
                        self.threshold
                    ),
                );
                true
            } else {
                false
            }
        }
    }

    impl Drop for TimingGuard {
        fn drop(&mut self) {
            if self.active {
                self.finish();
            }
        }
    }

    /// Create a timing guard with a **static** label (zero-allocation).
    /// Use this on all hot paths (update, view, `AnimTick`, subscription).
    pub fn perf_guard(
        level: Level,
        scope: &'static str,
        label: &'static str,
        threshold: Duration,
    ) -> TimingGuard {
        let active = enabled_at(level);
        TimingGuard {
            active,
            scope,
            label,
            dynamic_label: None,
            threshold,
            start: active.then(Instant::now),
            frame_id: if active {
                FRAME_COUNTER.load(Ordering::Relaxed)
            } else {
                0
            },
        }
    }

    /// Create a timing guard with a **dynamic** label (requires an owned
    /// String).  Only use on cold paths (file I/O, tar, etc.) where the
    /// allocation is negligible compared to the operation being measured.
    pub fn perf_guard_dyn(
        level: Level,
        scope: &'static str,
        label: String,
        threshold: Duration,
    ) -> TimingGuard {
        let active = enabled_at(level);
        TimingGuard {
            active,
            scope,
            label: "",
            dynamic_label: active.then_some(label),
            threshold,
            start: active.then(Instant::now),
            frame_id: if active {
                FRAME_COUNTER.load(Ordering::Relaxed)
            } else {
                0
            },
        }
    }

    /// Log a duration measurement immediately (non-RAII).  Useful when a
    /// guard-like pattern doesn't fit (e.g. async operations or early-return
    /// paths).
    #[allow(
        dead_code,
        reason = "kept for call sites that cannot hold a RAII guard"
    )]
    pub fn perf_log(
        level: Level,
        scope: &'static str,
        label: impl AsRef<str>,
        elapsed: Duration,
        threshold: Duration,
    ) {
        if !enabled_at(level) {
            return;
        }
        if elapsed >= threshold {
            let frame_id = FRAME_COUNTER.load(Ordering::Relaxed);
            log(
                level,
                scope,
                format!(
                    "[frame={}] {} took {:?} (threshold {:?})",
                    frame_id,
                    label.as_ref(),
                    elapsed,
                    threshold
                ),
            );
        }
    }

    // Called periodically (AnimTick) to prove the loop is alive: gaps larger
    // than ~16 ms in the log mean the main thread was blocked.

    #[allow(dead_code, reason = "kept as a manual liveness probe")]
    pub fn heartbeat() {
        // Cheap on purpose — one relaxed atomic load; the logging lives in
        // perf_guard/perf_log.
        let _ = enabled_at(Level::Trace);
    }

    // Counted so tests and the in-app log tab can surface slow messages.

    #[allow(dead_code, reason = "read through `slow_update_count`")]
    static SLOW_UPDATE_COUNT: AtomicU64 = AtomicU64::new(0);

    #[allow(dead_code, reason = "kept for the slow-update counter's API")]
    pub fn record_slow_update() {
        SLOW_UPDATE_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    #[allow(dead_code, reason = "kept for the slow-update counter's API")]
    pub fn slow_update_count() -> u64 {
        SLOW_UPDATE_COUNT.load(Ordering::Relaxed)
    }

    pub fn log_path() -> PathBuf {
        LOG_PATH
            .get()
            .cloned()
            .unwrap_or_else(|| paths::config_dir().join("terminal_diagnostics.log"))
    }

    fn rotate_if_needed(path: &PathBuf) {
        let Ok(metadata) = fs::metadata(path) else {
            return;
        };

        if metadata.len() <= MAX_LOG_BYTES {
            return;
        }

        let rotated = path.with_extension("log.1");
        let _ = fs::remove_file(&rotated);
        let _ = fs::rename(path, rotated);
    }
} // mod imp

pub use imp::*;
