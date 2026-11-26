// Podoxide
// Copyright (C) 2026  Stefan Stark
//
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option)
// any later version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE.  See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// this program.  If not, see <https://www.gnu.org/licenses/>.

//! Entry point for the Podoxide CLI tool (`pods`).

#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::ExitCode;

use podoxide::core::cli;
use podoxide::core::config::DEFAULT_CONFIG_PATH;
use podoxide::core::context::Context;
use podoxide::core::log;
use podoxide::core::modes;
use podoxide::error::{PodError, SystemError};
use podoxide::io::exec::command::ExecOutcome;
use podoxide::io::exec::podman;
use podoxide::io::system::env as system_env;

/// Runs the CLI pipeline for `arguments`, logs any error to standard error, and returns the process exit code.
fn execute<I, T>(arguments: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    finish_execution(run(arguments), &mut io::stderr().lock())
}

/// Maps a CLI run result to an [`ExitCode`], logging structured errors to `stderr` without duplicating output for [`ExecOutcome::Failed`].
fn finish_execution<E: Write>(result: Result<ExecOutcome, PodError>, stderr: &mut E) -> ExitCode {
    match result {
        Ok(ExecOutcome::Succeeded) => ExitCode::SUCCESS,
        Ok(ExecOutcome::Failed) => ExitCode::FAILURE,
        Err(error) => {
            log::write_pod_error(stderr, &error);
            ExitCode::FAILURE
        }
    }
}

/// Entry point of the `pods` binary that forwards process command-line arguments to [`execute`].
fn main() -> ExitCode {
    execute(env::args_os().skip(1))
}

/// Parses CLI arguments, verifies host environment prerequisites for the selected action, loads `pods.conf` when required, and dispatches the selected mode.
fn run<I, T>(arguments: I) -> Result<ExecOutcome, PodError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut context = Context::new();
    cli::parse_args(&mut context, arguments)?;
    log::set_debug_enabled(context.flags.debug());
    log::debug("Debug mode is enabled.");
    if context.config.path != Path::new(DEFAULT_CONFIG_PATH) {
        log::debug(&format!(
            "Config '{}' is used.",
            context.config.path.display()
        ));
    }

    verify_environment_for(&context)?;

    // Load pods.conf only when required by the selected mode and action.
    // Bypassed for help actions, version, init, and config show.
    context.load_config_if_required()?;
    modes::dispatch_mode(&context)
}

/// Verifies host OS, Podman `5.8.0+`, and elevated execution confirmation as required by `context`.
fn verify_environment_for(context: &Context) -> Result<(), PodError> {
    let mut stdin = io::stdin().lock();
    let mut stderr = io::stderr().lock();
    let podman_supported = !context.requires_podman() || podman::check_version();
    let elevated = context.requires_elevation_confirmation() && system_env::runs_elevated();
    verify_environment_with(
        context,
        system_env::check_os(),
        podman_supported,
        elevated,
        &mut stdin,
        &mut stderr,
    )
}

/// Evaluates OS, Podman version, and elevated-execution checks required by `context`, prompting on `reader` and `stderr_writer` when elevated confirmation is required.
fn verify_environment_with<R, E>(
    context: &Context,
    os_supported: bool,
    podman_supported: bool,
    elevated: bool,
    reader: &mut R,
    stderr_writer: &mut E,
) -> Result<(), PodError>
where
    R: BufRead,
    E: Write,
{
    if !os_supported {
        return Err(PodError::from(SystemError::UnsupportedOs));
    }
    if context.requires_podman() && !podman_supported {
        return Err(PodError::from(SystemError::UnsupportedPodmanVersion));
    }
    if context.requires_elevation_confirmation() && elevated {
        system_env::confirm_elevated_execution(reader, stderr_writer)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::io::{Cursor, Write};
    use std::os::unix::fs as unix_fs;
    use std::path::Path;
    use std::process::{self, Command, ExitCode, Stdio};

    use podoxide::core::context::{
        CommandAction, ConfigAction, ConnectAction, Context, DefaultAction, ImageAction,
        InitAction, LogsAction, ModeAction, RecipeAction,
    };
    use podoxide::error::{PodError, SystemError};
    use podoxide::io::exec::command::ExecOutcome;
    use super::{execute, finish_execution, run, verify_environment_with};

    const SUBPROCESS_MARKER: &str = "<<<PODOXIDE_CLI_OUTPUT_START>>>\n";

    fn assert_debug_custom_config_outputs(working_dir: &Path) {
        let custom_conf = working_dir.join("custom.conf");
        fs::write(
            &custom_conf,
            "general {\n    editor vim\n    simulate false\n}\nrecipes {\n    groups {\n        all recipe\n    }\n}\n",
        )
        .unwrap();

        let (false_ok, false_stdout, false_stderr) = run_cli_in_dir(
            working_dir,
            &["--debug", "--config=custom", "image", "prune"],
            b"no\n",
        );
        assert!(false_ok);
        assert_eq!(
            false_stderr,
            "DEBUG: Debug mode is enabled.\nDEBUG: Config './custom.conf' is used.\nDEBUG: Image mode is used.\n"
        );
        assert!(
            false_stdout == "No images found to prune.\n"
                || (false_stdout.starts_with("Are you sure you want to prune ")
                    && false_stdout.ends_with("? Type 'yes' to proceed: Aborted.\n"))
        );

        fs::write(
            &custom_conf,
            "general {\n    editor vim\n    simulate true\n}\nrecipes {\n    groups {\n        all recipe\n    }\n}\n",
        )
        .unwrap();

        let (true_ok, true_stdout, true_stderr) = run_cli_in_dir(
            working_dir,
            &["--debug", "--config=custom", "image", "prune"],
            b"",
        );
        assert!(true_ok);
        assert_eq!(
            true_stderr,
            "DEBUG: Debug mode is enabled.\nDEBUG: Config './custom.conf' is used.\nINFO: Simulate mode is active.\nDEBUG: Image mode is used.\n"
        );
        assert_eq!(true_stdout, "Prune images: \npodman image prune -f\n");
    }

    fn assert_missing_config_outputs(working_dir: &Path) {
        let bypass_cases: &[(&[&str], &str)] = &[
            (&[], "Usage: pods [MODE] [OPTIONS] ACTION [TARGETS]"),
            (&["help"], "Usage: pods [MODE] [OPTIONS] ACTION [TARGETS]"),
            (&["version"], "Podoxide v"),
            (&["cmd"], "Command Mode"),
            (&["cmd", "help"], "Command Mode"),
            (&["cfg", "help"], "Usage: pods config [OPTIONS] ACTION"),
            (&["con"], "Connect Mode"),
            (&["con", "help"], "Connect Mode"),
            (&["img"], "Image Mode"),
            (&["img", "help"], "Image Mode"),
            (&["init", "help"], "Usage: pods init [OPTIONS] [ACTION]"),
            (&["log"], "Logs Mode"),
            (&["log", "help"], "Logs Mode"),
            (&["rec", "help"], "Usage: pods recipe [OPTIONS] ACTION NAME"),
        ];

        for (arguments, expected_stdout) in bypass_cases {
            let (success, stdout, stderr) = run_cli_in_dir(working_dir, arguments, b"");
            assert!(
                success,
                "expected bypass {arguments:?} to succeed: {stderr}"
            );
            assert!(
                stdout.contains(expected_stdout),
                "expected {expected_stdout:?} in stdout for {arguments:?}, got {stdout:?}"
            );
            assert!(
                stderr.is_empty(),
                "expected empty stderr for bypass {arguments:?}, got {stderr:?}"
            );
        }

        let config_required_cases: &[&[&str]] = &[
            &["status"],
            &["up", "recipe"],
            &["down", "recipe"],
            &["create", "recipe"],
            &["remove", "recipe"],
            &["start", "recipe"],
            &["stop", "recipe"],
            &["restart", "recipe"],
            &["reload", "recipe"],
            &["update", "recipe"],
            &["cmd", "list", "recipe"],
            &["cmd", "exec", "recipe", "migrate"],
            &["cfg"],
            &["cfg", "inspect"],
            &["cfg", "show"],
            &["cfg", "edit"],
            &["con", "recipe/db"],
            &["img", "prune"],
            &["img", "prune", "--preview"],
            &["img", "update", "recipe"],
            &["log", "recipe"],
            &["log", "follow", "recipe"],
            &["rec"],
            &["rec", "list"],
            &["rec", "inspect", "recipe"],
            &["rec", "show", "recipe"],
            &["rec", "edit", "recipe"],
        ];

        for arguments in config_required_cases {
            let (success, stdout, stderr) = run_cli_in_dir(working_dir, arguments, b"");
            assert!(
                !success,
                "expected {arguments:?} to fail when pods.conf is missing"
            );
            assert!(
                stdout.is_empty(),
                "expected empty stdout for failed {arguments:?}, got {stdout:?}"
            );
            assert_eq!(
                stderr, "ERROR: Config error: configuration file not found: './pods.conf'\n",
                "unexpected stderr for {arguments:?}"
            );
        }
    }

    fn assert_simulate_false_and_flag_override_outputs(working_dir: &Path) {
        fs::write(
            working_dir.join("pods.conf"),
            "general {\n    editor true\n    simulate false\n}\npods {\n    path ./pods\n}\nrecipes {\n    path .\n    groups {\n        all recipe\n    }\n}\n",
        )
        .unwrap();

        let (prune_ok, prune_stdout, prune_stderr) =
            run_cli_in_dir(working_dir, &["image", "prune"], b"no\n");
        assert!(prune_ok);
        assert!(prune_stderr.is_empty());
        assert!(
            prune_stdout == "No images found to prune.\n"
                || (prune_stdout.starts_with("Are you sure you want to prune ")
                    && prune_stdout.ends_with("? Type 'yes' to proceed: Aborted.\n"))
        );

        let (prune_all_ok, prune_all_stdout, prune_all_stderr) =
            run_cli_in_dir(working_dir, &["image", "prune", "--all"], b"no\n");
        assert!(prune_all_ok);
        assert!(prune_all_stderr.is_empty());
        assert!(
            prune_all_stdout == "No images found to prune.\n"
                || (prune_all_stdout.starts_with("Are you sure you want to prune ")
                    && prune_all_stdout.ends_with("? Type 'yes' to proceed: Aborted.\n"))
        );

        let (cfg_ok, cfg_stdout, cfg_stderr) =
            run_cli_in_dir(working_dir, &["cfg", "inspect"], b"");
        assert!(cfg_ok);
        assert!(cfg_stderr.is_empty());
        assert!(cfg_stdout.contains("  Simulate:     false\n"));

        let (show_ok, show_stdout, show_stderr) =
            run_cli_in_dir(working_dir, &["cfg", "show"], b"");
        assert!(show_ok);
        assert!(show_stderr.is_empty());
        assert!(show_stdout.contains("  1: general {\n"));

        let (edit_ok, edit_stdout, edit_stderr) =
            run_cli_in_dir(working_dir, &["cfg", "edit"], b"");
        assert!(edit_ok);
        assert!(edit_stderr.is_empty());
        assert!(edit_stdout.is_empty());

        for (rec_args, expected_stdout) in [
            (&["rec", "list"][..], "Recipes:\n"),
            (
                &["rec", "inspect", "recipe"][..],
                "Recipe 'recipe' is valid.\n",
            ),
            (&["rec", "show", "recipe"][..], "  6: pod example-stack {\n"),
            (&["rec", "edit", "recipe"][..], ""),
            (&["status"][..], "example-stack-app"),
        ] {
            let (rec_ok, rec_stdout, rec_stderr) = run_cli_in_dir(working_dir, rec_args, b"");
            assert!(rec_ok);
            assert!(rec_stderr.is_empty());
            assert!(rec_stdout.contains(expected_stdout));
        }

        let flag_override_cases: &[(&[&str], &str)] = &[
            (
                &["--simulate", "image", "prune"],
                "Prune images: \npodman image prune -f\n",
            ),
            (
                &["--simulate", "image", "prune", "--all"],
                "Prune images: \npodman image prune -f -a\n",
            ),
            (&["--simulate", "cfg", "inspect"], "  Simulate:     true\n"),
            (&["--simulate", "cfg", "edit"], "true ./pods.conf\n"),
            (
                &["--simulate", "rec", "edit", "recipe"],
                "true ./recipe.pod\n",
            ),
            (
                &["--simulate", "up", "recipe"],
                "podman pod create --name example-stack --publish 8080:80/TCP --userns=keep-id\n",
            ),
            (
                &["--simulate", "cmd", "exec", "recipe", "migrate"],
                "podman exec -it -u dbuser example-stack-db db-cli -U dbuser -d app -f /migrations/run.sql\n",
            ),
            (
                &["--simulate", "con", "recipe/db"],
                "podman exec -it example-stack-db sh -c 'command -v bash >/dev/null 2>&1 && exec bash || exec sh'\n",
            ),
            (
                &["--simulate", "log", "show", "recipe"],
                "Execute log command: \npodman pod logs -n --color example-stack\n",
            ),
            (
                &["--simulate", "img", "update", "recipe"],
                "podman pull docker.io/example/db-server:latest\n",
            ),
        ];

        for (arguments, expected_stdout) in flag_override_cases {
            let (success, stdout, stderr) = run_cli_in_dir(working_dir, arguments, b"");
            assert!(success, "expected {arguments:?} to succeed: {stderr}");
            assert_eq!(stderr, "INFO: Simulate mode is active.\n");
            assert!(
                stdout.contains(expected_stdout),
                "expected {expected_stdout:?} in stdout for {arguments:?}, got {stdout:?}"
            );
        }

        let (sim_show_ok, sim_show_stdout, sim_show_stderr) =
            run_cli_in_dir(working_dir, &["--simulate", "cfg", "show"], b"");
        assert!(sim_show_ok);
        assert!(sim_show_stderr.is_empty());
        assert!(sim_show_stdout.contains("  1: general {\n"));

        assert_debug_custom_config_outputs(working_dir);
    }

    fn assert_simulate_true_outputs(working_dir: &Path) {
        let (sim_init_ok, sim_init_stdout, sim_init_stderr) =
            run_cli_in_dir(working_dir, &["init", "--simulate"], b"");
        assert!(sim_init_ok, "init --simulate failed: {sim_init_stderr}");
        assert!(sim_init_stdout.is_empty());
        assert!(sim_init_stderr.contains("INFO: Simulate mode is active.\n"));
        assert!(sim_init_stderr.contains("INFO: Created 'recipe.pod'.\n"));
        assert!(sim_init_stderr.contains("INFO: Created 'pods.conf'.\n"));
        assert!(!working_dir.join("recipe.pod").exists());
        assert!(!working_dir.join("pods.conf").exists());

        let (init_ok, init_stdout, init_stderr) = run_cli_in_dir(working_dir, &["init"], b"");
        assert!(init_ok, "init failed: {init_stderr}");
        assert!(init_stdout.is_empty());
        assert!(init_stderr.contains("INFO: Created 'recipe.pod'.\n"));
        assert!(init_stderr.contains("INFO: Created 'pods.conf'.\n"));
        assert!(!init_stderr.contains("Simulate mode is active."));

        let simulate_true_cases: &[(&[&str], &str)] = &[
            (
                &["up", "recipe"],
                "podman pod create --name example-stack --publish 8080:80/TCP --userns=keep-id\n",
            ),
            (&["down", "recipe"], "podman pod rm example-stack\n"),
            (
                &["create", "recipe"],
                "podman create --name example-stack-db --pod example-stack",
            ),
            (
                &["create", "recipe/db"],
                "podman pod create --name example-stack --publish 8080:80/TCP --userns=keep-id\nCreate container 'example-stack-db': \npodman create --name example-stack-db --pod example-stack",
            ),
            (&["remove", "recipe"], "podman rm example-stack-db\n"),
            (&["start", "recipe"], "podman start example-stack-db\n"),
            (&["stop", "recipe"], "podman pod stop example-stack\n"),
            (&["stop", "recipe/db"], "podman stop example-stack-db\n"),
            (&["stop", "recipe/1"], "podman stop example-stack-db\n"),
            (&["stop", "recipe/*app"], "podman stop example-stack-app\n"),
            (
                &["stop", "recipe/example-stack-app"],
                "podman stop example-stack-app\n",
            ),
            (&["restart", "recipe"], "podman start example-stack-app\n"),
            (&["reload", "recipe"], "podman pod rm example-stack\n"),
            (
                &["update", "recipe"],
                "Update Summary:\n  Simulated (1):\n    - example-stack (Example Web Stack)\n\nSimulated 1 pod (2 containers).\n",
            ),
            (
                &["update", "recipe/db"],
                "Update Summary:\n  Simulated (1):\n    - example-stack-db (docker.io/example/db-server:latest)\n\nSimulated 1 container.\n",
            ),
            (
                &["cmd", "list", "recipe"],
                "Commands for recipe 'recipe':\n",
            ),
            (
                &["cmd", "exec", "recipe", "migrate"],
                "podman exec -it -u dbuser example-stack-db db-cli -U dbuser -d app -f /migrations/run.sql\n",
            ),
            (&["cfg", "inspect"], "  Simulate:     true\n"),
            (&["cfg", "edit"], "vim ./pods.conf\n"),
            (
                &["con", "recipe/db"],
                "podman exec -it example-stack-db sh -c 'command -v bash >/dev/null 2>&1 && exec bash || exec sh'\n",
            ),
            (&["img", "prune"], "Prune images: \npodman image prune -f\n"),
            (
                &["img", "prune", "--all"],
                "Prune images: \npodman image prune -f -a\n",
            ),
            (
                &["img", "update", "recipe"],
                "Update Summary:\n  Simulated (1):\n    - example-stack (Example Web Stack)\n\nSimulated 1 pod (2 images).\n",
            ),
            (
                &["img", "update", "recipe/db"],
                "Update Summary:\n  Simulated (1):\n    - example-stack-db (docker.io/example/db-server:latest)\n\nSimulated 1 image.\n",
            ),
            (
                &["log", "show", "recipe"],
                "Execute log command: \npodman pod logs -n --color example-stack\n",
            ),
            (
                &["log", "follow", "recipe/db"],
                "Execute log command: \npodman pod logs -n --color -f -c example-stack-db example-stack\n",
            ),
            (&["status"], "example-stack-app"),
            (&["rec", "list"], "Recipes:\n"),
            (&["rec", "inspect", "recipe"], "Recipe 'recipe' is valid.\n"),
            (&["rec", "show", "recipe"], "  6: pod example-stack {\n"),
            (&["rec", "edit", "recipe"], "vim ./recipe.pod\n"),
        ];

        for (arguments, expected_stdout) in simulate_true_cases {
            let (success, stdout, stderr) = run_cli_in_dir(working_dir, arguments, b"");
            assert!(success, "expected {arguments:?} to succeed: {stderr}");
            assert_eq!(
                stderr, "INFO: Simulate mode is active.\n",
                "missing or unexpected simulate info on stderr for {arguments:?}"
            );
            assert!(
                stdout.contains(expected_stdout),
                "expected {expected_stdout:?} in stdout for {arguments:?}, got {stdout:?}"
            );
        }

        let (show_ok, show_stdout, show_stderr) =
            run_cli_in_dir(working_dir, &["cfg", "show"], b"");
        assert!(show_ok);
        assert!(show_stderr.is_empty());
        assert!(show_stdout.contains("  5: general {\n"));
    }

    #[test]
    fn binary_in_usr_local_bin_uses_current_working_directory() {
        if env::var_os("PODOXIDE_TEST_SUBPROCESS_CWD").is_some() {
            assert_eq!(execute(["status"]), ExitCode::FAILURE);
            assert_eq!(execute(["init"]), ExitCode::SUCCESS);
            assert_eq!(execute(["init"]), ExitCode::SUCCESS);
            assert_eq!(execute(["status"]), ExitCode::SUCCESS);
            assert_eq!(execute(["up", "recipe"]), ExitCode::SUCCESS);
            assert_eq!(execute(["cfg", "show"]), ExitCode::SUCCESS);
            assert_eq!(execute(["rec", "list"]), ExitCode::SUCCESS);
            return;
        }

        let temp_dir = env::temp_dir().join(format!("podoxide_cwd_test_{}", process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        let bin_dir = temp_dir.join("usr").join("local").join("bin");
        let working_dir = temp_dir.join("workspace");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&working_dir).unwrap();

        let installed_pods = bin_dir.join("pods");
        unix_fs::symlink(env::current_exe().unwrap(), &installed_pods).unwrap();

        let output = Command::new(&installed_pods)
            .current_dir(&working_dir)
            .env("PODOXIDE_TEST_SUBPROCESS_CWD", "1")
            .args([
                "--exact",
                "tests::binary_in_usr_local_bin_uses_current_working_directory",
                "--nocapture",
            ])
            .output()
            .unwrap();
        let stdout_output = String::from_utf8_lossy(&output.stdout);
        let stderr_output = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "subprocess failed: {stderr_output}"
        );
        let canonical_working_dir = fs::canonicalize(&working_dir).unwrap();
        assert!(
            stderr_output
                .contains("ERROR: Config error: configuration file not found: './pods.conf'")
        );
        assert!(stderr_output.contains(&format!(
            "INFO: Initializing workspace '{}'.",
            canonical_working_dir.display()
        )));
        assert!(stderr_output.contains("INFO: Created 'recipe.pod'."));
        assert!(stderr_output.contains("INFO: Created 'pods.conf'."));
        assert!(stderr_output.contains("WARNING: File 'recipe.pod' already exists."));
        assert!(stderr_output.contains("WARNING: File 'pods.conf' already exists."));
        assert!(stdout_output.contains(
            "podman pod create --name example-stack --publish 8080:80/TCP --userns=keep-id"
        ));
        assert!(stdout_output.contains("podman start example-stack-db"));
        assert!(stdout_output.contains("podman start example-stack-app"));
        assert!(working_dir.join("recipe.pod").is_file());
        assert!(working_dir.join("pods.conf").is_file());
        assert!(!bin_dir.join("recipe.pod").exists());
        assert!(!bin_dir.join("pods.conf").exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn cli_output_matrix_verifies_simulate_true_false_flag_and_missing_config() {
        if let Ok(encoded_args) = env::var("PODOXIDE_TEST_CLI_RUNNER") {
            let arguments: Vec<&str> = if encoded_args.is_empty() {
                Vec::new()
            } else {
                encoded_args.split('\n').collect()
            };
            print!("{SUBPROCESS_MARKER}");
            let _ = std::io::stdout().flush();
            let exit_code = execute(arguments);
            let _ = std::io::stdout().flush();
            let _ = std::io::stderr().flush();
            process::exit(i32::from(exit_code == ExitCode::FAILURE));
        }

        let temp_dir = env::temp_dir().join(format!("podoxide_matrix_test_{}", process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        assert_missing_config_outputs(&temp_dir);
        assert_simulate_true_outputs(&temp_dir);
        assert_simulate_false_and_flag_override_outputs(&temp_dir);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn execute_pipeline_handles_valid_and_invalid_arguments() {
        assert_eq!(execute(Vec::<String>::new()), ExitCode::SUCCESS);
        assert_eq!(execute(["help"]), ExitCode::SUCCESS);
        assert_eq!(execute(["version"]), ExitCode::SUCCESS);
        assert_eq!(
            execute(["--debug", "--config=pods", "up", "recipe"]),
            ExitCode::SUCCESS
        );
        assert_eq!(
            execute(["--debug", "--config=staging", "up", "recipe"]),
            ExitCode::FAILURE
        );
        assert_eq!(execute(["up", "recipe"]), ExitCode::SUCCESS);
        assert_eq!(execute(["up", "@all"]), ExitCode::SUCCESS);
        assert_eq!(execute(["up", "recipe/db", "recipe/2"]), ExitCode::SUCCESS);
        assert_eq!(execute(["up", "recipe", "recipe/db"]), ExitCode::FAILURE);
        assert_eq!(execute(["up", "example-app"]), ExitCode::FAILURE);
        assert_eq!(execute(["cmd", "list", "recipe"]), ExitCode::SUCCESS);
        assert_eq!(execute(["cfg", "inspect"]), ExitCode::SUCCESS);
        assert_eq!(execute(["cfg", "show"]), ExitCode::SUCCESS);
        assert_eq!(execute(["con", "recipe/db"]), ExitCode::SUCCESS);
        assert_eq!(execute(["con", "recipe"]), ExitCode::FAILURE);
        assert_eq!(execute(["img", "prune"]), ExitCode::SUCCESS);
        assert_eq!(execute(["img", "update", "recipe"]), ExitCode::SUCCESS);
        assert_eq!(execute(["img", "update", "recipe/db"]), ExitCode::SUCCESS);
        assert_eq!(execute(["init", "help"]), ExitCode::SUCCESS);
        assert_eq!(execute(["init", "--simulate"]), ExitCode::SUCCESS);
        assert_eq!(execute(["init"]), ExitCode::SUCCESS);
        assert_eq!(execute(["init", "--config=staging"]), ExitCode::FAILURE);
        assert_eq!(execute(["log", "recipe"]), ExitCode::SUCCESS);
        assert_eq!(execute(["rec", "list"]), ExitCode::SUCCESS);
        assert_eq!(execute(["rec", "inspect", "recipe"]), ExitCode::SUCCESS);

        assert_eq!(execute(["--unknown-flag"]), ExitCode::FAILURE);
        assert_eq!(execute(["ini"]), ExitCode::FAILURE);
        assert!(run(["up"]).is_err());
    }

    fn run_cli_in_dir(
        working_dir: &Path,
        arguments: &[&str],
        stdin_bytes: &[u8],
    ) -> (bool, String, String) {
        let mut child = Command::new(env::current_exe().unwrap())
            .current_dir(working_dir)
            .env("PODOXIDE_TEST_CLI_RUNNER", arguments.join("\n"))
            .args([
                "--exact",
                "tests::cli_output_matrix_verifies_simulate_true_false_flag_and_missing_config",
                "--nocapture",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(mut child_stdin) = child.stdin.take() {
            let _ = child_stdin.write_all(stdin_bytes);
        }
        let output = child.wait_with_output().unwrap();
        let raw_stdout = String::from_utf8_lossy(&output.stdout);
        let stdout = raw_stdout
            .split_once(SUBPROCESS_MARKER)
            .map_or(raw_stdout.as_ref(), |(_, after)| after)
            .to_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        (output.status.success(), stdout, stderr)
    }

    #[test]
    fn finish_execution_maps_outcomes_and_avoids_duplicate_error_on_failed_outcome() {
        let mut ok_stderr = Vec::new();
        assert_eq!(
            finish_execution(Ok(ExecOutcome::Succeeded), &mut ok_stderr),
            ExitCode::SUCCESS
        );
        assert!(ok_stderr.is_empty());

        let mut failed_stderr = Vec::new();
        assert_eq!(
            finish_execution(Ok(ExecOutcome::Failed), &mut failed_stderr),
            ExitCode::FAILURE
        );
        assert!(
            failed_stderr.is_empty(),
            "expected no duplicate ERROR line on ExecOutcome::Failed, got {failed_stderr:?}"
        );

        let mut err_stderr = Vec::new();
        assert_eq!(
            finish_execution(
                Err(PodError::from(SystemError::UnsupportedOs)),
                &mut err_stderr
            ),
            ExitCode::FAILURE
        );
        assert_eq!(
            String::from_utf8(err_stderr).unwrap(),
            "ERROR: Only Linux is supported.\n"
        );
    }

    #[test]
    fn verify_environment_enforces_os_podman_and_elevation_checks() {
        let mut empty_input = Cursor::new(Vec::<u8>::new());
        let mut stderr = Vec::new();

        // Help and version actions bypass Podman version and elevated-execution checks.
        let bypass_actions = [
            ModeAction::Command(CommandAction::Help),
            ModeAction::Config(ConfigAction::Help),
            ModeAction::Connect(ConnectAction::Help),
            ModeAction::Default(DefaultAction::Help),
            ModeAction::Default(DefaultAction::Version),
            ModeAction::Image(ImageAction::Help),
            ModeAction::Init(InitAction::Help),
            ModeAction::Logs(LogsAction::Help),
            ModeAction::Recipe(RecipeAction::Help),
        ];
        for action in bypass_actions {
            let mut context = Context::new();
            context.action = action;
            let mut prompt_err = Vec::new();
            assert!(
                verify_environment_with(
                    &context,
                    true,
                    false,
                    true,
                    &mut empty_input,
                    &mut prompt_err
                )
                .is_ok(),
                "expected {action:?} to succeed without Podman or root confirmation"
            );
            assert!(
                prompt_err.is_empty(),
                "expected no root warning prompt for {action:?}"
            );
            let os_err = verify_environment_with(
                &context,
                false,
                false,
                false,
                &mut empty_input,
                &mut prompt_err,
            )
            .unwrap_err();
            assert_eq!(os_err.to_string(), "Only Linux is supported.");
        }

        // `pods init` bypasses Podman version check while still checking OS and elevated confirmation.
        let mut init_context = Context::new();
        init_context.action = ModeAction::Init(InitAction::Create);
        assert!(
            verify_environment_with(
                &init_context,
                true,
                false,
                false,
                &mut empty_input,
                &mut stderr
            )
            .is_ok()
        );
        let init_os_err = verify_environment_with(
            &init_context,
            false,
            false,
            false,
            &mut empty_input,
            &mut stderr,
        )
        .unwrap_err();
        assert_eq!(init_os_err.to_string(), "Only Linux is supported.");
        let mut init_yes = Cursor::new(b"yes\n".to_vec());
        assert!(
            verify_environment_with(&init_context, true, false, true, &mut init_yes, &mut stderr)
                .is_ok()
        );
        let mut init_no = Cursor::new(b"no\n".to_vec());
        let init_abort =
            verify_environment_with(&init_context, true, false, true, &mut init_no, &mut stderr)
                .unwrap_err();
        assert_eq!(init_abort.to_string(), "Aborting execution.");

        // Operational actions require Linux OS, Podman 5.8.0+, and elevation confirmation.
        let mut up_context = Context::new();
        up_context.action = ModeAction::Default(DefaultAction::Up);

        let os_error = verify_environment_with(
            &up_context,
            false,
            true,
            false,
            &mut empty_input,
            &mut stderr,
        )
        .unwrap_err();
        assert_eq!(os_error.to_string(), "Only Linux is supported.");

        let podman_error = verify_environment_with(
            &up_context,
            true,
            false,
            false,
            &mut empty_input,
            &mut stderr,
        )
        .unwrap_err();
        assert_eq!(
            podman_error.to_string(),
            "Podman 5.8.0 or higher is required."
        );

        assert!(
            verify_environment_with(
                &up_context,
                true,
                true,
                false,
                &mut empty_input,
                &mut stderr
            )
            .is_ok()
        );

        let mut yes_input = Cursor::new(b"yes\n".to_vec());
        assert!(
            verify_environment_with(&up_context, true, true, true, &mut yes_input, &mut stderr)
                .is_ok()
        );

        let mut no_input = Cursor::new(b"no\n".to_vec());
        let abort_error =
            verify_environment_with(&up_context, true, true, true, &mut no_input, &mut stderr)
                .unwrap_err();
        assert_eq!(abort_error.to_string(), "Aborting execution.");
    }

    #[test]
    fn custom_pods_conf_logs_debug_path_and_broken_config_shows_lines() {
        let temp_dir = env::temp_dir().join(format!("podoxide_custom_conf_test_{}", process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        let sub_dir = temp_dir.join("nested");
        fs::create_dir_all(&sub_dir).unwrap();

        let nested_conf = sub_dir.join("pods.conf");
        fs::write(&nested_conf, "broken {{{ syntax\nline two\n").unwrap();
        let config_arg = format!("--config={}", nested_conf.display());

        let (show_ok, show_stdout, show_stderr) =
            run_cli_in_dir(&temp_dir, &["--debug", &config_arg, "cfg", "show"], b"");
        assert!(
            show_ok,
            "expected cfg show to succeed on broken pods.conf: {show_stderr}"
        );
        assert!(show_stdout.contains("  1: broken {{{ syntax\n"));
        assert!(show_stdout.contains("  2: line two\n"));
        assert!(
            show_stderr.contains(&format!(
                "DEBUG: Config '{}' is used.\n",
                nested_conf.display()
            )),
            "expected debug log for custom pods.conf path, got: {show_stderr:?}"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
