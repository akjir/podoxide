//! Direct operating system process execution helpers.

use std::process::Command;

/// Executes `program` with `arguments` directly and returns captured `stdout` when exit status is 0.
#[must_use]
pub fn exec_capture(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program).args(arguments).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

#[cfg(test)]
mod tests {
    use super::exec_capture;

    #[test]
    fn exec_capture_handles_success_failure_and_missing_binary() {
        let output = exec_capture("echo", &["podoxide-test"]).unwrap();
        assert_eq!(output.trim(), "podoxide-test");

        assert!(exec_capture("false", &[]).is_none());
        assert!(exec_capture("/nonexistent/podoxide_missing_bin", &[]).is_none());
    }
}
