use std::{
    ffi::CString,
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use tokio::{
    fs::OpenOptions,
    io::{AsyncBufReadExt, BufReader},
    task,
};

use tokio::sync::mpsc;

use crate::{
    AnyResult,
    commands::{Command, parse_command},
};

pub async fn serve_control_pipe(
    socket_path: PathBuf,
    sender: mpsc::Sender<Command>,
) -> AnyResult<()> {
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

        let Some(cmd) = parse_command(line.trim()) else {
            continue;
        };

        let _ = sender.send(cmd).await;
    }
}

fn create_fifo(path: &Path) -> std::io::Result<()> {
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
