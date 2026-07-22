use std::{
    fs,
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use template_app::DiagnosticEventStore;
use tracing_subscriber::{
    Layer, filter::filter_fn, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};

const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
const MAX_REPORT_EVENTS: u32 = 20_000;

pub struct DiagnosticsReportText {
    pub title: String,
    pub version: String,
    pub generated: String,
    pub privacy: String,
    pub events: String,
    pub no_events: String,
}

#[derive(Clone)]
pub struct DiagnosticsController {
    directory: Arc<PathBuf>,
    enabled: Arc<AtomicBool>,
    export_in_flight: Arc<AtomicBool>,
    store: Arc<dyn DiagnosticEventStore>,
}

impl DiagnosticsController {
    pub fn without_logger(
        directory: PathBuf,
        enabled: bool,
        store: Arc<dyn DiagnosticEventStore>,
    ) -> Self {
        Self {
            directory: Arc::new(directory),
            enabled: Arc::new(AtomicBool::new(enabled)),
            export_in_flight: Arc::new(AtomicBool::new(false)),
            store,
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn begin_export(&self) -> bool {
        self.export_in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn finish_export(&self) {
        self.export_in_flight.store(false, Ordering::Release);
    }

    pub fn export_report(
        &self,
        destination: &Path,
        text: &DiagnosticsReportText,
    ) -> Result<(), io::Error> {
        let mut report = format!(
            "{}\n{}\n{}\n{}\n\n{}\n",
            text.title, text.version, text.generated, text.privacy, text.events
        );
        let empty_report = report.clone();

        let database_events = self
            .store
            .diagnostic_events(MAX_REPORT_EVENTS)
            .unwrap_or_default();
        let events = match read_disk_events(&self.directory) {
            Ok(disk_events) => merge_event_sequences(disk_events, database_events),
            Err(_) if !database_events.is_empty() => database_events,
            Err(error) => return Err(error),
        };
        append_stored_events(&mut report, events);

        if report == empty_report {
            report.push_str(&text.no_events);
            report.push('\n');
        }
        write_private_report(destination, report.as_bytes())
    }
}

pub fn init(
    directory: PathBuf,
    enabled: bool,
    store: Arc<dyn DiagnosticEventStore>,
) -> Result<DiagnosticsController, io::Error> {
    let controller = DiagnosticsController::without_logger(directory.clone(), enabled, store);
    prepare_directory(&directory)?;
    let writer = SanitizedLogWriter::new(
        BoundedLogWriter::open(directory)?,
        Arc::clone(&controller.store),
    );
    let filter_enabled = Arc::clone(&controller.enabled);
    let log_layer = fmt::layer()
        .with_ansi(false)
        .with_target(false)
        .compact()
        .with_writer(Mutex::new(writer))
        .with_filter(filter_fn(move |metadata| {
            filter_enabled.load(Ordering::Relaxed) && metadata.fields().field("event").is_some()
        }));
    tracing_subscriber::registry()
        .with(log_layer)
        .try_init()
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(controller)
}

#[cfg(test)]
fn append_safe_events(report: &mut String, log: &str) {
    report.push_str(&safe_report_lines(log));
}

fn read_disk_events(directory: &Path) -> Result<Vec<String>, io::Error> {
    let mut events = Vec::new();
    for path in [
        directory.join("diagnostics.log.previous"),
        directory.join("diagnostics.log"),
    ] {
        match fs::read_to_string(path) {
            Ok(log) => events.extend(safe_events(&log).map(str::to_owned)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(events)
}

fn merge_event_sequences(mut disk: Vec<String>, database: Vec<String>) -> Vec<String> {
    if database.ends_with(&disk) {
        return database;
    }
    if disk.ends_with(&database) {
        return disk;
    }
    let overlap = (1..=disk.len().min(database.len()))
        .rev()
        .find(|&count| disk[disk.len() - count..] == database[..count])
        .unwrap_or_default();
    disk.extend(database.into_iter().skip(overlap));
    disk
}

fn append_stored_events(report: &mut String, events: Vec<String>) {
    for event in events {
        if is_safe_event(&event) {
            report.push_str("- ");
            report.push_str(&event);
            report.push('\n');
        }
    }
}

#[cfg(test)]
fn safe_report_lines(log: &str) -> String {
    let mut events = String::new();
    for event in safe_events(log) {
        events.push_str("- ");
        events.push_str(event);
        events.push('\n');
    }
    events
}

fn safe_events(log: &str) -> impl Iterator<Item = &str> {
    log.lines()
        .filter_map(|line| field_value(line, "event"))
        .filter(|event| is_safe_event(event))
}

fn field_value<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("{name}=");
    let raw = line
        .split_whitespace()
        .find_map(|part| part.strip_prefix(&marker))?;
    let value = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(raw);
    (!value.is_empty()).then_some(value)
}

fn is_safe_event(value: &str) -> bool {
    value.len() <= 120
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn write_private_report(destination: &Path, contents: &[u8]) -> Result<(), io::Error> {
    let mut file = open_private_report(destination)?;
    file.write_all(contents)?;
    file.flush()?;
    restrict_file_permissions(destination)
}

fn prepare_directory(directory: &Path) -> Result<(), io::Error> {
    fs::create_dir_all(directory)?;
    restrict_directory_permissions(directory)
}

struct BoundedLogWriter {
    current: PathBuf,
    previous: PathBuf,
    file: File,
    bytes_written: u64,
}

struct SanitizedLogWriter {
    inner: BoundedLogWriter,
    store: Arc<dyn DiagnosticEventStore>,
}

impl SanitizedLogWriter {
    fn new(inner: BoundedLogWriter, store: Arc<dyn DiagnosticEventStore>) -> Self {
        Self { inner, store }
    }
}

impl Write for SanitizedLogWriter {
    fn write(&mut self, buffer: &[u8]) -> Result<usize, io::Error> {
        if let Ok(log) = std::str::from_utf8(buffer) {
            for line in log.lines() {
                let Some(event) = field_value(line, "event").filter(|event| is_safe_event(event))
                else {
                    continue;
                };
                writeln!(self.inner, "event={event}")?;
                let _ = self.store.record_diagnostic_event(event);
            }
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        self.inner.flush()
    }
}

impl BoundedLogWriter {
    fn open(directory: PathBuf) -> Result<Self, io::Error> {
        let current = directory.join("diagnostics.log");
        let previous = directory.join("diagnostics.log.previous");
        let file = open_log(&current)?;
        let bytes_written = file.metadata()?.len();
        let mut writer = Self {
            current,
            previous,
            file,
            bytes_written,
        };
        if writer.bytes_written >= MAX_LOG_BYTES {
            writer.rotate()?;
        }
        Ok(writer)
    }

    fn rotate(&mut self) -> Result<(), io::Error> {
        self.file.flush()?;
        if self.previous.exists() {
            fs::remove_file(&self.previous)?;
        }
        fs::rename(&self.current, &self.previous)?;
        self.file = open_log(&self.current)?;
        self.bytes_written = 0;
        Ok(())
    }
}

impl Write for BoundedLogWriter {
    fn write(&mut self, buffer: &[u8]) -> Result<usize, io::Error> {
        if self.bytes_written > 0
            && self.bytes_written.saturating_add(buffer.len() as u64) > MAX_LOG_BYTES
        {
            self.rotate()?;
        }
        let written = self.file.write(buffer)?;
        self.bytes_written = self.bytes_written.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        self.file.flush()
    }
}

fn open_log(path: &Path) -> Result<File, io::Error> {
    let file = open_private_log(path)?;
    restrict_file_permissions(path)?;
    Ok(file)
}

#[cfg(target_os = "macos")]
fn open_private_report(path: &Path) -> Result<File, io::Error> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(target_os = "macos"))]
fn open_private_report(path: &Path) -> Result<File, io::Error> {
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
}

#[cfg(target_os = "macos")]
fn open_private_log(path: &Path) -> Result<File, io::Error> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(target_os = "macos"))]
fn open_private_log(path: &Path) -> Result<File, io::Error> {
    OpenOptions::new().create(true).append(true).open(path)
}

#[cfg(target_os = "macos")]
fn restrict_directory_permissions(path: &Path) -> Result<(), io::Error> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(target_os = "macos"))]
fn restrict_directory_permissions(_path: &Path) -> Result<(), io::Error> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn restrict_file_permissions(path: &Path) -> Result<(), io::Error> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(target_os = "macos"))]
fn restrict_file_permissions(_path: &Path) -> Result<(), io::Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    #[cfg(target_os = "macos")]
    use std::os::unix::fs::PermissionsExt;

    static TEST_ID: AtomicUsize = AtomicUsize::new(0);

    #[derive(Default)]
    struct MemoryDiagnosticStore {
        events: Mutex<Vec<String>>,
        fail_reads: AtomicBool,
    }

    impl DiagnosticEventStore for MemoryDiagnosticStore {
        fn record_diagnostic_event(&self, event: &str) -> Result<(), template_app::StorageError> {
            self.events
                .lock()
                .map(|mut events| events.push(event.to_owned()))
                .map_err(|_| template_app::StorageError::Unavailable("test lock poisoned".into()))
        }

        fn diagnostic_events(&self, limit: u32) -> Result<Vec<String>, template_app::StorageError> {
            if self.fail_reads.load(Ordering::Relaxed) {
                return Err(template_app::StorageError::Unavailable(
                    "injected read failure".into(),
                ));
            }
            self.events
                .lock()
                .map(|events| {
                    let start = events.len().saturating_sub(limit as usize);
                    events[start..].to_vec()
                })
                .map_err(|_| template_app::StorageError::Unavailable("test lock poisoned".into()))
        }
    }

    #[test]
    fn rotates_one_bounded_previous_log() {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let directory =
            env::temp_dir().join(format!("saymore-diagnostics-{}-{id}", std::process::id()));
        assert!(prepare_directory(&directory).is_ok());
        let Ok(mut writer) = BoundedLogWriter::open(directory.clone()) else {
            panic!("test log should open");
        };
        assert!(
            writer
                .write_all(&vec![b'x'; MAX_LOG_BYTES as usize - 10])
                .is_ok()
        );
        assert!(writer.write_all(&[b'y'; 20]).is_ok());
        assert!(writer.flush().is_ok());

        let Ok(current) = directory.join("diagnostics.log").metadata() else {
            panic!("current log should have metadata");
        };
        let Ok(previous) = directory.join("diagnostics.log.previous").metadata() else {
            panic!("previous log should have metadata");
        };
        assert_eq!(20, current.len());
        assert_eq!(MAX_LOG_BYTES - 10, previous.len());
        #[cfg(target_os = "macos")]
        let Ok(directory_mode) = directory
            .metadata()
            .map(|metadata| metadata.permissions().mode())
        else {
            panic!("diagnostics directory should have permissions");
        };
        #[cfg(target_os = "macos")]
        {
            assert_eq!(0o700, directory_mode & 0o777);
            assert_eq!(0o600, current.permissions().mode() & 0o777);
            assert_eq!(0o600, previous.permissions().mode() & 0o777);
        }
        assert!(fs::remove_dir_all(directory).is_ok());
    }

    #[test]
    fn report_keeps_only_safe_event_identifiers() {
        let mut report = String::new();
        append_safe_events(
            &mut report,
            "INFO event=recording.started api_key=secret transcript=private\nWARN event=\"asr.provider_rejected\" text=private\nINFO event=bad/value token=secret",
        );

        assert_eq!("- recording.started\n- asr.provider_rejected\n", report);
        assert!(!report.contains("secret"));
        assert!(!report.contains("private"));
    }

    #[test]
    fn report_merge_keeps_order_without_repeating_shared_events() {
        assert_eq!(
            vec!["old".to_owned(), "shared".to_owned(), "new".to_owned()],
            merge_event_sequences(
                vec!["old".to_owned(), "shared".to_owned()],
                vec!["shared".to_owned(), "new".to_owned()]
            )
        );
    }

    #[test]
    fn sanitized_writer_never_persists_sensitive_fields() {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let directory = env::temp_dir().join(format!(
            "saymore-diagnostics-sanitized-{}-{id}",
            std::process::id()
        ));
        assert!(prepare_directory(&directory).is_ok());
        let Ok(writer) = BoundedLogWriter::open(directory.clone()) else {
            panic!("test log should open");
        };
        let store = Arc::new(MemoryDiagnosticStore::default());
        let mut writer = SanitizedLogWriter::new(writer, store.clone());

        assert!(
            writer
                .write_all(b"INFO event=asr.failed api_key=secret transcript=private\n")
                .is_ok()
        );
        assert!(writer.flush().is_ok());
        let Ok(log) = fs::read_to_string(directory.join("diagnostics.log")) else {
            panic!("test log should be readable");
        };
        assert_eq!("event=asr.failed\n", log);
        assert_eq!(
            vec!["asr.failed".to_owned()],
            store.diagnostic_events(10).unwrap_or_default()
        );
        assert!(fs::remove_dir_all(directory).is_ok());
    }

    #[test]
    fn subscriber_captures_targetless_event_and_strips_other_fields() {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let directory = env::temp_dir().join(format!(
            "saymore-diagnostics-subscriber-{}-{id}",
            std::process::id()
        ));
        assert!(prepare_directory(&directory).is_ok());
        let Ok(writer) = BoundedLogWriter::open(directory.clone()) else {
            panic!("test log should open");
        };
        let store = Arc::new(MemoryDiagnosticStore::default());
        let writer = SanitizedLogWriter::new(writer, store.clone());
        let layer = fmt::layer()
            .with_ansi(false)
            .with_target(false)
            .compact()
            .with_writer(Mutex::new(writer))
            .with_filter(filter_fn(|metadata| {
                metadata.fields().field("event").is_some()
            }));
        let subscriber = tracing_subscriber::registry().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(event = "settings.save_failed", reason = "private detail");
            tracing::warn!(reason = "missing event");
        });

        let log = fs::read_to_string(directory.join("diagnostics.log")).unwrap_or_default();
        assert_eq!("event=settings.save_failed\n", log);
        assert_eq!(
            vec!["settings.save_failed".to_owned()],
            store.diagnostic_events(10).unwrap_or_default()
        );
        assert!(fs::remove_dir_all(directory).is_ok());
    }

    #[test]
    fn report_uses_database_events_and_falls_back_to_disk() {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let directory = env::temp_dir().join(format!(
            "saymore-diagnostics-report-{}-{id}",
            std::process::id()
        ));
        assert!(prepare_directory(&directory).is_ok());
        assert!(fs::write(directory.join("diagnostics.log"), "event=disk.fallback\n").is_ok());
        let store = Arc::new(MemoryDiagnosticStore::default());
        assert!(store.record_diagnostic_event("database.primary").is_ok());
        let controller =
            DiagnosticsController::without_logger(directory.clone(), true, store.clone());
        let text = report_text();
        let database_report = directory.join("database-report.txt");
        assert!(controller.export_report(&database_report, &text).is_ok());
        let database_contents = fs::read_to_string(database_report).unwrap_or_default();
        assert!(database_contents.contains("- database.primary"));
        assert!(database_contents.contains("- disk.fallback"));

        store.fail_reads.store(true, Ordering::Relaxed);
        let fallback_report = directory.join("fallback-report.txt");
        assert!(controller.export_report(&fallback_report, &text).is_ok());
        let fallback_contents = fs::read_to_string(fallback_report).unwrap_or_default();
        assert!(fallback_contents.contains("- disk.fallback"));
        assert!(fs::remove_dir_all(directory).is_ok());
    }

    fn report_text() -> DiagnosticsReportText {
        DiagnosticsReportText {
            title: "Report".into(),
            version: "Version".into(),
            generated: "Generated".into(),
            privacy: "Privacy".into(),
            events: "Events".into(),
            no_events: "None".into(),
        }
    }
}
