use std::{ffi::CString, fs, os::unix::ffi::OsStrExt, path::PathBuf, sync::Arc};

use tokio::{
    fs::OpenOptions,
    io::{AsyncBufReadExt, BufReader},
    task,
};

use crate::{
    AnyResult,
    crabbox::{Command, Crabbox},
};

pub async fn serve_control_pipe(socket_path: PathBuf, crabbox: Arc<Crabbox>) -> AnyResult<()> {
    if socket_path.exists() {
        fs::remove_file(&socket_path)?;
    }

    task::spawn_blocking({
        let path = socket_path.clone();
        move || create_fifo(&path)
    })
    .await??;

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&socket_path)
        .await?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();

    loop {
        line.clear();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            continue;
        }

        if let Some(cmd) = parse_command(line.trim()) {
            let _ = crabbox.sender().send(cmd).await;
        }
    }
}

fn parse_command(input: &str) -> Option<Command> {
    match input.trim().to_ascii_uppercase().as_str() {
        "PLAY" => Some(Command::Play),
        "STOP" => Some(Command::Stop),
        _ => None,
    }
}

fn create_fifo(path: &PathBuf) -> std::io::Result<()> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
    let mode = 0o666;
    let res = unsafe { libc::mkfifo(c_path.as_ptr(), mode) };
    if res == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
