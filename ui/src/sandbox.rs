use std::fs::File;
use std::io::prelude::*;
use std::io::{BufReader, BufWriter, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::Command;

use mktemp::Temp;

pub struct Sandbox {
    scratch_dir: Temp,
}

impl Sandbox {
    pub fn new() -> Self {
        Sandbox {
            scratch_dir: Temp::new_dir().expect("Unable to create temp dir"),
        }
    }

    pub fn compile(&self, req: &CompileRequest) -> CompileResponse {
        self.write_source_code(&req.code);

        let mut output_path = self.scratch_dir.as_ref().to_path_buf();
        output_path.push("compiler-output");
        let mut command = self.compile_command(&req.target, &req.channel, &req.mode, req.tests);

        let output = command.output().expect("Failed to run");

        CompileResponse {
            success: output.status.success(),
            code: read(&output_path).unwrap_or_else(String::new),
            stdout: String::from_utf8(output.stdout).expect("Stdout was not UTF-8"),
            stderr: String::from_utf8(output.stderr).expect("Stderr was not UTF-8"),
        }
    }

    pub fn execute(&self, req: &ExecuteRequest) -> ExecuteResponse {
        self.write_source_code(&req.code);
        let mut command = self.execute_command(&req.channel, &req.mode, req.tests);

        let output = command.output().expect("Failed to run");

        ExecuteResponse {
            success: output.status.success(),
            stdout: String::from_utf8(output.stdout).expect("Stdout was not UTF-8"),
            stderr: String::from_utf8(output.stderr).expect("Stderr was not UTF-8"),
        }
    }

    pub fn format(&self, req: &FormatRequest) -> FormatResponse {
        let path = self.write_source_code(&req.code);
        let mut command = self.format_command();

        let output = command.output().expect("Failed to run");

        FormatResponse {
            success: output.status.success(),
            code: read(path.as_path()).expect("No formatting output"),
            stdout: String::from_utf8(output.stdout).expect("Stdout was not UTF-8"),
            stderr: String::from_utf8(output.stderr).expect("Stderr was not UTF-8"),
        }
    }

    fn write_source_code(&self, code: &str) -> PathBuf {
        let data = code.as_bytes();

        let path = {
            let mut p = self.scratch_dir.to_path_buf();
            p.push("main.rs");
            p
        };

        let file = File::create(&path).expect("Unable to create source code");
        let mut file = BufWriter::new(file);

        file.write_all(data).expect("Unable to write source code");

        debug!("Wrote {} bytes of source to {}", data.len(), path.display());
        path
    }

    fn compile_command(&self, target: &str, channel: &str, mode: &str, tests: bool) -> Command {
        let container = format!("rust-{}", channel);
        let mut cmd = self.docker_command();

        let execution_cmd = match (target, mode, tests) {
            ("llvm-ir", "debug", false) => r#"rustc main.rs -o compiler-output --emit llvm-ir"#,
            ("llvm-ir", "debug", true) => r#"rustc --test main.rs -o compiler-output --emit llvm-ir"#,
            ("llvm-ir", "release", false) => r#"rustc -C opt-level=3 main.rs -o compiler-output --emit llvm-ir"#,
            ("llvm-ir", "release", true) => r#"rustc -C opt-level=3 --test main.rs -o compiler-output --emit llvm-ir"#,
            ("asm", "debug", false) => r#"rustc main.rs -o compiler-output --emit asm"#,
            ("asm", "debug", true) => r#"rustc --test main.rs -o compiler-output --emit asm"#,
            ("asm", "release", false) => r#"rustc -C opt-level=3 main.rs -o compiler-output --emit asm"#,
            ("asm", "release", true) => r#"rustc -C opt-level=3 --test main.rs -o compiler-output --emit asm"#,
            other => panic!("Unknown configuration: {:?}", other),
        };

        cmd.arg(&container).args(&["bash", "-c", execution_cmd]);

        debug!("Compilation command is {:?}", cmd);

        cmd
    }

    fn execute_command(&self, channel: &str, mode: &str, tests: bool) -> Command {
        let container = format!("rust-{}", channel);
        let mut cmd = self.docker_command();

        let execution_cmd = match (mode, tests) {
            ("debug", false) => r#"rustc main.rs && ./main"#,
            ("debug", true) => r#"rustc --test main.rs && ./main"#,
            ("release", false) => r#"rustc -C opt-level=3 main.rs && ./main"#,
            ("release", true) => r#"rustc -C opt-level=3 --test main.rs && ./main"#,
            other => panic!("Unknown configuration: {:?}", other),
        };

        cmd.arg(&container).args(&["bash", "-c", execution_cmd]);

        debug!("Execution command is {:?}", cmd);

        cmd
    }

    fn format_command(&self) -> Command {
        let mut cmd = self.docker_command();

        cmd.arg("rustfmt").args(&["main.rs"]);

        debug!("Formatting command is {:?}", cmd);

        cmd
    }

    fn docker_command(&self) -> Command {
        const DIR_INSIDE_CONTAINER: &'static str = "/source";

        let utf8_dir = self.scratch_dir.as_ref().to_str().expect("Unable to convert directory to UTF-8");
        let mount_source_volume = format!("{}:{}", utf8_dir, DIR_INSIDE_CONTAINER);

        let mut cmd = Command::new("docker");

        cmd
            .arg("run")
            .args(&["--volume", &mount_source_volume])
            .args(&["--workdir", DIR_INSIDE_CONTAINER]);

        cmd
    }
}

fn read(path: &Path) -> Option<String> {
    let f = match File::open(path) {
        Ok(f) => f,
        Err(ref e) if e.kind() == ErrorKind::NotFound => return None,
        Err(e) => panic!("Couldn't open file {}: {}", path.display(), e),
    };
    let mut f = BufReader::new(f);

    let mut s = String::new();
    f.read_to_string(&mut s).expect("Couldn't read");
    Some(s)
}

#[derive(Debug, Clone)]
pub struct CompileRequest {
    pub target: String,
    pub channel: String,
    pub mode: String,
    pub tests: bool,
    pub code: String,
}

#[derive(Debug, Clone)]
pub struct CompileResponse {
    pub success: bool,
    pub code: String,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone)]
pub struct ExecuteRequest {
    pub channel: String,
    pub mode: String,
    pub tests: bool,
    pub code: String,
}

#[derive(Debug, Clone)]
pub struct ExecuteResponse {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone)]
pub struct FormatRequest {
    pub code: String,
}

#[derive(Debug, Clone)]
pub struct FormatResponse {
    pub success: bool,
    pub code: String,
    pub stdout: String,
    pub stderr: String,
}
