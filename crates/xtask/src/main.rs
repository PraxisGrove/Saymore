use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[cfg(target_os = "macos")]
mod macos_bundle;

const DEFAULT_WARN_FILE_LINES: usize = 600;
const DEFAULT_MAX_FILE_LINES: usize = 800;
const DEFAULT_WARN_FN_LINES: usize = 80;
const DEFAULT_MAX_FN_LINES: usize = 150;
const README_DEPENDENCY_NAME: &str = "template";

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("[ERROR] {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    let Some(command) = args.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };

    match command {
        #[cfg(target_os = "macos")]
        "bundle-macos" => macos_bundle::run(),
        "size" => run_size_gate(SizeConfig::from_args(&args[1..])?),
        "update-readme-version" => update_readme_version(&args[1..]),
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown xtask command: {other}").into()),
    }
}

fn print_help() {
    println!("xtask commands:");
    #[cfg(target_os = "macos")]
    println!("  bundle-macos");
    println!(
        "  size [--root <dir>] [--glob <glob>] [--warn-file-lines <n>] [--max-file-lines <n>] [--warn-fn-lines <n>] [--max-fn-lines <n>]"
    );
    println!("  update-readme-version <version>");
}

#[derive(Debug, Clone)]
struct SizeConfig {
    root: PathBuf,
    globs: Vec<String>,
    warn_file_lines: usize,
    max_file_lines: usize,
    warn_fn_lines: usize,
    max_fn_lines: usize,
}

impl Default for SizeConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            globs: vec!["crates/**/*.rs".to_owned(), "apps/**/*.rs".to_owned()],
            warn_file_lines: DEFAULT_WARN_FILE_LINES,
            max_file_lines: DEFAULT_MAX_FILE_LINES,
            warn_fn_lines: DEFAULT_WARN_FN_LINES,
            max_fn_lines: DEFAULT_MAX_FN_LINES,
        }
    }
}

impl SizeConfig {
    fn from_args(args: &[String]) -> Result<Self, Box<dyn Error>> {
        let mut config = Self::default();
        let mut index = 0;

        while index < args.len() {
            match args[index].as_str() {
                "--root" => {
                    config.root = PathBuf::from(required_value(args, index)?);
                    index += 2;
                }
                "--glob" => {
                    if config.globs == Self::default().globs {
                        config.globs.clear();
                    }
                    config.globs.push(required_value(args, index)?.to_owned());
                    index += 2;
                }
                "--warn-file-lines" => {
                    config.warn_file_lines = required_value(args, index)?.parse()?;
                    index += 2;
                }
                "--max-file-lines" => {
                    config.max_file_lines = required_value(args, index)?.parse()?;
                    index += 2;
                }
                "--warn-fn-lines" => {
                    config.warn_fn_lines = required_value(args, index)?.parse()?;
                    index += 2;
                }
                "--max-fn-lines" => {
                    config.max_fn_lines = required_value(args, index)?.parse()?;
                    index += 2;
                }
                other => return Err(format!("unknown size option: {other}").into()),
            }
        }

        Ok(config)
    }
}

fn required_value(args: &[String], index: usize) -> Result<&str, Box<dyn Error>> {
    args.get(index + 1)
        .map(String::as_str)
        .ok_or_else(|| format!("missing value for {}", args[index]).into())
}

#[derive(Debug)]
struct FileFinding {
    relpath: String,
    lines: usize,
}

#[derive(Debug)]
struct FunctionFinding {
    relpath: String,
    name: String,
    start_line: usize,
    body_lines: usize,
}

fn run_size_gate(config: SizeConfig) -> Result<(), Box<dyn Error>> {
    let root = config.root.canonicalize()?;
    let files = collect_rust_files(&root, &config.globs)?;

    let mut file_warnings = Vec::new();
    let mut file_errors = Vec::new();
    let mut fn_warnings = Vec::new();
    let mut fn_errors = Vec::new();

    for path in &files {
        let relpath = path
            .strip_prefix(&root)?
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(path)?;
        let lines: Vec<&str> = source.lines().collect();
        let line_count = lines.len();

        if line_count > config.max_file_lines {
            file_errors.push(FileFinding {
                relpath: relpath.clone(),
                lines: line_count,
            });
        } else if line_count > config.warn_file_lines {
            file_warnings.push(FileFinding {
                relpath: relpath.clone(),
                lines: line_count,
            });
        }

        for finding in iter_rust_functions(&lines, &relpath) {
            if finding.body_lines > config.max_fn_lines {
                fn_errors.push(finding);
            } else if finding.body_lines > config.warn_fn_lines {
                fn_warnings.push(finding);
            }
        }
    }

    println!("[INFO] root={}", root.display());
    println!("[INFO] scanned_files={}", files.len());
    println!(
        "[INFO] file_warn>{} file_max>{}",
        config.warn_file_lines, config.max_file_lines
    );
    println!(
        "[INFO] fn_warn>{} fn_max>{}",
        config.warn_fn_lines, config.max_fn_lines
    );

    print_file_findings("[WARN] oversized files", &file_warnings);
    print_file_findings("[ERROR] oversized files", &file_errors);
    print_fn_findings("[WARN] oversized functions (approx)", &fn_warnings);
    print_fn_findings("[ERROR] oversized functions (approx)", &fn_errors);

    let warning_count = file_warnings.len() + fn_warnings.len();
    let error_count = file_errors.len() + fn_errors.len();
    println!();
    println!("[SUMMARY] warnings={warning_count} errors={error_count}");

    if error_count > 0 {
        return Err("size gate failed".into());
    }

    Ok(())
}

fn collect_rust_files(root: &Path, globs: &[String]) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();
    collect_files(root, root, globs, &mut files)?;
    files.sort();
    files.dedup();
    Ok(files)
}

fn collect_files(
    root: &Path,
    current: &Path,
    globs: &[String],
    files: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        if name == ".git" || name == "target" || name == ".codegraph" {
            continue;
        }

        if path.is_dir() {
            collect_files(root, &path, globs, files)?;
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }

        let relpath = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if globs.iter().any(|glob| matches_glob(glob, &relpath)) {
            files.push(path);
        }
    }

    Ok(())
}

fn matches_glob(pattern: &str, relpath: &str) -> bool {
    match pattern {
        "crates/**/*.rs" => relpath.starts_with("crates/") && relpath.ends_with(".rs"),
        "apps/**/*.rs" => relpath.starts_with("apps/") && relpath.ends_with(".rs"),
        "**/*.rs" => relpath.ends_with(".rs"),
        exact => relpath == exact,
    }
}

fn iter_rust_functions(lines: &[&str], relpath: &str) -> Vec<FunctionFinding> {
    let mut findings = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        let Some(fn_pos) = line.find("fn ") else {
            index += 1;
            continue;
        };

        let name = extract_fn_name(&line[fn_pos + 3..]);
        if name.is_empty() {
            index += 1;
            continue;
        }

        let start_line = index + 1;
        let mut scan_index = index;
        let mut body_open_line = None;

        while scan_index < lines.len() {
            let scan_line = lines[scan_index];
            let semicolon = scan_line.find(';');
            let brace = scan_line.find('{');
            if semicolon.is_some() && (brace.is_none() || semicolon < brace) {
                break;
            }
            if brace.is_some() {
                body_open_line = Some(scan_index);
                break;
            }
            scan_index += 1;
        }

        let Some(body_open_line) = body_open_line else {
            index += 1;
            continue;
        };

        let mut brace_depth = 0isize;
        let mut end_index = body_open_line;
        for (offset, body_line) in lines[body_open_line..].iter().enumerate() {
            brace_depth += body_line.matches('{').count() as isize;
            brace_depth -= body_line.matches('}').count() as isize;
            if brace_depth == 0 {
                end_index = body_open_line + offset;
                break;
            }
        }

        findings.push(FunctionFinding {
            relpath: relpath.to_owned(),
            name,
            start_line,
            body_lines: end_index - body_open_line + 1,
        });

        index = (end_index + 1).max(index + 1);
    }

    findings
}

fn extract_fn_name(input: &str) -> String {
    input
        .chars()
        .take_while(|ch| *ch == '_' || ch.is_ascii_alphanumeric())
        .collect()
}

fn print_file_findings(title: &str, findings: &[FileFinding]) {
    if findings.is_empty() {
        return;
    }

    println!();
    println!("{title}");
    for finding in findings {
        println!("{}  {}", finding.lines, finding.relpath);
    }
}

fn print_fn_findings(title: &str, findings: &[FunctionFinding]) {
    if findings.is_empty() {
        return;
    }

    println!();
    println!("{title}");
    for finding in findings {
        println!(
            "{}  {}:{}  {}",
            finding.body_lines, finding.relpath, finding.start_line, finding.name
        );
    }
}

fn update_readme_version(args: &[String]) -> Result<(), Box<dyn Error>> {
    let Some(version) = args.first() else {
        return Err("usage: cargo run -p xtask -- update-readme-version <version>".into());
    };

    let readme = Path::new("README.md");
    let source = fs::read_to_string(readme)?;
    let updated = source
        .lines()
        .map(|line| update_readme_version_line(line, version))
        .collect::<Vec<_>>()
        .join("\n");

    fs::write(readme, format!("{updated}\n"))?;
    println!("[INFO] updated README.md version snippets to {version}");
    Ok(())
}

fn update_readme_version_line(line: &str, version: &str) -> String {
    let plain_prefix = format!("{README_DEPENDENCY_NAME} = \"");
    if line.trim_start().starts_with(&plain_prefix) {
        return format!("{README_DEPENDENCY_NAME} = \"{version}\"");
    }

    let inline_prefix = [README_DEPENDENCY_NAME, " = ", "{", " version = \""].concat();
    if line.contains(&inline_prefix) {
        return format!(
            "{README_DEPENDENCY_NAME} = {open} version = \"{version}\" {close}",
            open = "{",
            close = "}"
        );
    }

    line.to_owned()
}
