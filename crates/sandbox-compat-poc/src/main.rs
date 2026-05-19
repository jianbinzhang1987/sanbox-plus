use serde::Deserialize;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::time::Instant;

#[derive(Debug, Deserialize)]
struct Manifest {
    apps: Vec<AppCase>,
}

#[derive(Debug, Deserialize)]
struct AppCase {
    id: String,
    name: String,
    exe_path: PathBuf,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    working_dir: Option<PathBuf>,
    #[serde(default = "default_timeout_seconds")]
    timeout_seconds: u64,
    #[serde(default)]
    notes: Option<String>,
}

fn default_timeout_seconds() -> u64 {
    10
}

fn main() -> ExitCode {
    let manifest_path = match parse_args(env::args().skip(1)) {
        Ok(path) => path,
        Err(message) => {
            eprintln!("{message}");
            eprintln!("Usage: sandbox-compat-poc <manifest.json>");
            return ExitCode::from(2);
        }
    };

    match run(&manifest_path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("POC-0105 failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<PathBuf, String> {
    let mut args = args.into_iter();
    let value = args
        .next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "manifest path is required".to_string())?;
    if args.next().is_some() {
        return Err("only one manifest path is accepted".to_string());
    }
    Ok(PathBuf::from(value))
}

fn run(manifest_path: &PathBuf) -> Result<(), String> {
    let text = fs::read_to_string(manifest_path)
        .map_err(|error| format!("failed to read manifest: {error}"))?;
    let manifest: Manifest =
        serde_json::from_str(&text).map_err(|error| format!("invalid manifest: {error}"))?;

    if manifest.apps.is_empty() {
        return Err("manifest must contain at least one app".into());
    }

    println!("POC-0105 compatibility run");
    for app in manifest.apps {
        run_case(&app)?;
    }
    Ok(())
}

fn run_case(app: &AppCase) -> Result<(), String> {
    if !app.exe_path.exists() {
        println!("{} ({}) skipped: exe_path does not exist", app.id, app.name);
        return Ok(());
    }

    println!("starting {} ({})", app.id, app.name);
    if let Some(notes) = &app.notes {
        println!("notes: {notes}");
    }

    let mut command = Command::new(&app.exe_path);
    command.args(&app.args);
    if let Some(working_dir) = &app.working_dir {
        command.current_dir(working_dir);
    }

    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to start {}: {error}", app.id))?;
    println!("{} started pid={}", app.id, child.id());
    println!(
        "{} manual observation window: {} seconds",
        app.id, app.timeout_seconds
    );
    std::thread::sleep(std::time::Duration::from_secs(app.timeout_seconds));

    match child.try_wait() {
        Ok(Some(status)) => println!("{} exited early with {status}", app.id),
        Ok(None) => {
            println!(
                "{} still running after {} ms; terminating",
                app.id,
                started.elapsed().as_millis()
            );
            let _ = child.kill();
        }
        Err(error) => println!("{} status check failed: {error}", app.id),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_manifest_arg() {
        assert!(parse_args(Vec::<String>::new()).is_err());
    }

    #[test]
    fn parses_manifest_json() {
        let json = r#"{
            "apps": [{
                "id": "oa",
                "name": "OA",
                "exe_path": "C:\\Program Files\\OA\\oa.exe"
            }]
        }"#;
        let result = serde_json::from_str::<Manifest>(json);
        assert!(result.is_ok(), "{result:?}");
        let Ok(manifest) = result else { return };
        assert_eq!(manifest.apps.len(), 1);
        assert_eq!(manifest.apps[0].timeout_seconds, 10);
    }
}
