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
