use std::{
    env, fs,
    fs::{File, OpenOptions, Permissions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

use tracing_subscriber::{
    Layer, filter::filter_fn, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};

const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

pub fn init() -> Result<(), io::Error> {
    let directory = diagnostics_directory()?;
    prepare_directory(&directory)?;
    let writer = BoundedLogWriter::open(directory)?;
    let log_layer = fmt::layer()
        .with_ansi(false)
        .with_target(false)
        .compact()
        .with_writer(Mutex::new(writer))
        .with_filter(filter_fn(|metadata| {
            metadata.target() == "saymore::diagnostics"
        }));
    tracing_subscriber::registry()
        .with(log_layer)
        .try_init()
        .map_err(|error| io::Error::other(error.to_string()))
}

fn prepare_directory(directory: &Path) -> Result<(), io::Error> {
    fs::create_dir_all(directory)?;
    fs::set_permissions(directory, Permissions::from_mode(0o700))
}

fn diagnostics_directory() -> Result<PathBuf, io::Error> {
    let home = env::var_os("HOME")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not defined"))?;
    Ok(PathBuf::from(home).join("Library/Application Support/Saymore/logs"))
}

struct BoundedLogWriter {
    current: PathBuf,
    previous: PathBuf,
    file: File,
    bytes_written: u64,
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
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    fs::set_permissions(path, Permissions::from_mode(0o600))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

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
        let Ok(directory_mode) = directory
            .metadata()
            .map(|metadata| metadata.permissions().mode())
        else {
            panic!("diagnostics directory should have permissions");
        };
        assert_eq!(0o700, directory_mode & 0o777);
        assert_eq!(0o600, current.permissions().mode() & 0o777);
        assert_eq!(0o600, previous.permissions().mode() & 0o777);
        assert!(fs::remove_dir_all(directory).is_ok());
    }
}
