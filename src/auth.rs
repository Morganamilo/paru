use anyhow::{Result, ensure, bail, Context};
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::FromRawFd;
use nix::unistd::pipe;
use crate::config::Config;
use std::process::Command;
use std::cell::RefCell;

#[derive(Debug)]
pub struct Pipe {
    pub read: File,
    pub write: File,
}

impl Pipe {
    fn wait_ok(&mut self) -> Result<()> {
        let mut buf = [0; "ok\n".len()];
        self.read.read_exact(&mut buf)?;
        ensure!(&buf == b"ok\n");
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct LazyPipe {
    pipe: RefCell<Option<Result<Pipe>>>,
}

impl LazyPipe {
    pub fn run(&self, config: &Config) -> Result<()> {
        let mut pipe = self.pipe.borrow_mut();
        let pipe = pipe.get_or_insert_with(|| spawn_auth(config).context("failed to spawn auth process"));
        let pipe = match pipe {
            Err(e) => bail!(e.to_string()),
            Ok(p) => p,
        };
        loop {}
        pipe.write.write_all(b"something")?;
        pipe.wait_ok()?;
        Ok(())
    }
}

pub fn spawn_auth(config: &Config) -> Result<Pipe> {
    let (paru_read, auth_write) = pipe()?;
    let (auth_read, paru_write) = pipe()?;


    /*Command::new(&config.sudo_bin)
        .args(&config.sudo_flags)
        .arg(std::env::current_exe()?)
        .arg("--authpipe")
        .arg(auth_read.to_string())
        .arg(auth_write.to_string())
        .spawn()?;*/
    Command::new(std::env::current_exe()?)
        .arg("--authpipe")
        .arg(auth_read.to_string())
        .arg(auth_write.to_string())
        .spawn()?;

    loop {}

    let read = unsafe { File::from_raw_fd(paru_read) };
    let write = unsafe { File::from_raw_fd(paru_write) };

    let mut pipe = Pipe { read, write };

    pipe.wait_ok().unwrap();
    Ok(pipe)
}

pub unsafe fn authpipe(read: &str, write: &str) -> Result<i32> {
    let read = read.parse::<i32>()?;
    let write = write.parse::<i32>()?;
    ensure!(read > 3);
    ensure!(write > 3);
    println!("read={} write={}", read, write);
    ensure!(read != write);

    loop {}

    let mut read = File::from_raw_fd(read);
    let mut write = File::from_raw_fd(write);


    loop {
        write.write(b"ok\n").context("failed to write ok")?;
        write.flush().context("failed to flush")?;
    }
    Ok(0)
}
