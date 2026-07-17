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

use tracing_subscriber::{
    Layer, filter::filter_fn, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};

const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

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
}

impl DiagnosticsController {
    pub fn without_logger(directory: PathBuf, enabled: bool) -> Self {
        Self {
            directory: Arc::new(directory),
            enabled: Arc::new(AtomicBool::new(enabled)),
            export_in_flight: Arc::new(AtomicBool::new(false)),
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

        for path in [
            self.directory.join("diagnostics.log.previous"),
            self.directory.join("diagnostics.log"),
        ] {
            match fs::read_to_string(path) {
                Ok(log) => append_safe_events(&mut report, &log),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }

        if report == empty_report {
            report.push_str(&text.no_events);
            report.push('\n');
        }
        write_private_report(destination, report.as_bytes())
    }
}

pub fn init(directory: PathBuf, enabled: bool) -> Result<DiagnosticsController, io::Error> {
    let controller = DiagnosticsController::without_logger(directory.clone(), enabled);
    prepare_directory(&directory)?;
    let writer = SanitizedLogWriter::new(BoundedLogWriter::open(directory)?);
    let filter_enabled = Arc::clone(&controller.enabled);
    let log_layer = fmt::layer()
        .with_ansi(false)
        .with_target(false)
        .compact()
        .with_writer(Mutex::new(writer))
        .with_filter(filter_fn(move |metadata| {
            filter_enabled.load(Ordering::Relaxed) && metadata.target() == "saymore::diagnostics"
        }));
    tracing_subscriber::registry()
        .with(log_layer)
        .try_init()
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(controller)
}

fn append_safe_events(report: &mut String, log: &str) {
    report.push_str(&safe_report_lines(log));
}

fn safe_report_lines(log: &str) -> String {
    let mut events = String::new();
    for line in log.lines() {
        let Some(event) = field_value(line, "event") else {
            continue;
        };
        if is_safe_event(event) {
            events.push_str("- ");
            events.push_str(event);
            events.push('\n');
        }
    }
    events
}

fn safe_log_lines(log: &str) -> String {
    let mut events = String::new();
    for line in log.lines() {
        let Some(event) = field_value(line, "event") else {
            continue;
        };
        if is_safe_event(event) {
            events.push_str("event=");
            events.push_str(event);
            events.push('\n');
        }
    }
    events
}

fn field_value<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("{name}=");
    let value = line
        .split_whitespace()
        .find_map(|part| part.strip_prefix(&marker))?;
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
}

impl SanitizedLogWriter {
    fn new(inner: BoundedLogWriter) -> Self {
        Self { inner }
    }
}

impl Write for SanitizedLogWriter {
    fn write(&mut self, buffer: &[u8]) -> Result<usize, io::Error> {
        if let Ok(log) = std::str::from_utf8(buffer) {
            let events = safe_log_lines(log);
            if !events.is_empty() {
                self.inner.write_all(events.as_bytes())?;
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
            "INFO event=recording.started api_key=secret transcript=private\nWARN event=asr.provider_rejected text=private\nINFO event=bad/value token=secret",
        );

        assert_eq!("- recording.started\n- asr.provider_rejected\n", report);
        assert!(!report.contains("secret"));
        assert!(!report.contains("private"));
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
        let mut writer = SanitizedLogWriter::new(writer);

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
        assert!(fs::remove_dir_all(directory).is_ok());
    }
}
