use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const AUTHOR: &str = "Rohan Adwankar <rohan.adwankar@gmail.com>";

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let args: Vec<OsString> = env::args_os().skip(1).collect();

    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return exec_cargo_init(&args);
    }

    let target = detect_target(&args);
    let status = Command::new("cargo").arg("init").args(&args).status()?;
    if !status.success() {
        return Ok(ExitCode::from(status.code().unwrap_or(1) as u8));
    }

    let manifest_dir = fs::canonicalize(&target)?;
    let manifest_path = manifest_dir.join("Cargo.toml");
    let repo_name = manifest_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Could not determine repo name"))?;

    rewrite_manifest(&manifest_path, repo_name)?;
    ensure_gitattributes(&manifest_dir)?;
    Ok(ExitCode::SUCCESS)
}

fn exec_cargo_init(args: &[OsString]) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let status = Command::new("cargo").arg("init").args(args).status()?;
    Ok(ExitCode::from(status.code().unwrap_or(1) as u8))
}

fn detect_target(args: &[OsString]) -> PathBuf {
    let mut target = PathBuf::from(".");
    let mut expect_value = false;

    for arg in args {
        if expect_value {
            expect_value = false;
            continue;
        }

        if let Some(arg) = arg.to_str() {
            match arg {
                "--vcs" | "--edition" | "--name" | "--registry" | "--color" | "--config" | "-Z" => {
                    expect_value = true;
                }
                _ if arg.starts_with("--vcs=")
                    || arg.starts_with("--edition=")
                    || arg.starts_with("--name=")
                    || arg.starts_with("--registry=")
                    || arg.starts_with("--color=")
                    || arg.starts_with("--config=") => {}
                "--bin" | "--lib" | "--locked" | "--offline" | "--frozen" | "-v" | "-vv" | "-vvv" | "-q" => {}
                _ if arg.starts_with('-') => {}
                _ => target = PathBuf::from(arg),
            }
        } else {
            target = PathBuf::from(arg);
        }
    }

    target
}

fn rewrite_manifest(manifest_path: &Path, repo_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let text = fs::read_to_string(manifest_path)?;
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();

    let (package_start, package_end) = find_section(&lines, "package")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Could not find [package] section in Cargo.toml"))?;

    let mut package_lines = Vec::new();
    for line in &lines[package_start + 1..package_end] {
        let trimmed = line.trim_start();
        if trimmed.starts_with("authors =")
            || trimmed.starts_with("description =")
            || trimmed.starts_with("repository =")
            || trimmed.starts_with("license =")
            || trimmed.starts_with("keywords =")
        {
            continue;
        }
        package_lines.push(line.clone());
    }

    package_lines.push(format!("authors = [\"{}\"]", escape_toml_string(AUTHOR)));
    package_lines.push(format!("description = \"{}\"", escape_toml_string(repo_name)));
    package_lines.push(format!(
        "repository = \"https://github.com/RohanAdwankar/{}\"",
        escape_toml_string(repo_name)
    ));
    package_lines.push(String::from("license = \"MIT\""));
    package_lines.push(format!("keywords = [\"{}\", \"devtool\"]", escape_toml_string(repo_name)));

    lines.splice(package_start + 1..package_end, package_lines);

    if let Some((clippy_start, clippy_end)) = find_section(&lines, "lints.clippy") {
        let mut clippy_lines = Vec::new();
        let mut found_all = false;

        for line in &lines[clippy_start + 1..clippy_end] {
            let trimmed = line.trim_start();
            if trimmed.starts_with("all =") {
                if !found_all {
                    clippy_lines.push(String::from("all = \"warn\""));
                    found_all = true;
                }
                continue;
            }
            clippy_lines.push(line.clone());
        }

        if !found_all {
            clippy_lines.push(String::from("all = \"warn\""));
        }

        lines.splice(clippy_start + 1..clippy_end, clippy_lines);
    } else {
        if lines.last().is_some_and(|line| !line.is_empty()) {
            lines.push(String::new());
        }
        lines.push(String::from("[lints.clippy]"));
        lines.push(String::from("all = \"warn\""));
    }

    let mut output = lines.join("\n");
    output.push('\n');
    fs::write(manifest_path, output)?;
    Ok(())
}

fn ensure_gitattributes(manifest_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let gitattributes_path = manifest_dir.join(".gitattributes");
    let entry = "tests/** linguist-vendored";

    let existing = match fs::read_to_string(&gitattributes_path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(Box::new(err)),
    };

    if existing.lines().any(|line| line.trim() == entry) {
        return Ok(());
    }

    let mut output = existing;
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(entry);
    output.push('\n');

    fs::write(gitattributes_path, output)?;
    Ok(())
}

fn find_section(lines: &[String], section: &str) -> Option<(usize, usize)> {
    let header = format!("[{section}]");
    let mut start = None;

    for (index, line) in lines.iter().enumerate() {
        if line.trim() == header {
            start = Some(index);
            continue;
        }

        if let Some(section_start) = start {
            if index > section_start && line.starts_with('[') {
                return Some((section_start, index));
            }
        }
    }

    start.map(|section_start| (section_start, lines.len()))
}

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
