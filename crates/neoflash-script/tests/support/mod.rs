use std::io::{self, Write};
use std::process::{Command, Output, Stdio};

pub fn run_node_from_stdin(source: &str, extra_args: &[&str]) -> io::Result<Output> {
    let mut child = Command::new("node")
        .args(extra_args)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "Node stdin was not piped"))?;
    stdin.write_all(source.as_bytes())?;
    drop(stdin);
    child.wait_with_output()
}
