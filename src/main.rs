//! Spawns N threads, with each thread writing a bash script then executing that same bash script.
//! The threads operate on completely separate files, so ideally they shouldn't interfere with each
//! others operations.
//! 
//! Running this with a single thread works fine. The program writes a bash script, runs it, then
//! repeats indefinitely. However, at least on my laptop running a 6.9.3 Linux kernel, running with
//! 2 or more threads results in almost immediate failure with the error `Text file busy (os error
//! 26)`. This happens despite the fact that each thread is working with a separate file. Just the
//! fact that the threads are writing then executing a file at the same time as at least one other
//! thread is executing anything is enough to trigger this.
//! 
//! The problem happens because when a subprocess is created (with clone3), it copies all open file
//! descriptors. When it then calls execve, the file descriptor is closed because `O_CLOEXEC` is
//! set. However, in that brief window in between creating the new subprocess and calling execve, we
//! have an extra copy of the file descriptor and that can mess with whatever thread was working
//! with that file.
//! 
//! It's also worth noting that the man page for clone3 says that if CLONE_VFORK is used, that the
//! calling process is suspended until the child process calls execve or _exit. If Linux actually
//! suspended the whole of the calling process, this problem wouldn't occur, however it actually
//! only suspends the calling thread.
//! 
//! Probably the cleanest fix for this would be if Linux had a don't-clone bit on file descriptors
//! that prevented them being duplicated by calls to `clone3`.
//! 
//! An easy workaround for this is to not execute the script, but instead execute bash and pass the
//! script as an argument. This sidesteps Linux's file locking, making it so that it doesn't matter
//! that another process still has the file open for write.

use anyhow::Context;
use std::io::Write;
use std::os::unix::prelude::PermissionsExt;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args();
    args.next();
    let (Some(base_path), Some(num_threads)) = (args.next(), args.next()) else {
        eprintln!("Expected arguments: {{temporary directory}} {{num threads}}");
        std::process::exit(1);
    };
    let base_path = Path::new(&base_path);
    let num_threads = num_threads.parse().context("Invalid num-threads")?;

    std::thread::scope(|scope| {
        for i in 0..num_threads {
            let script_path = base_path.join(i.to_string());

            scope.spawn(move || loop {
                if let Err(error) = create_and_execute_script(&script_path) {
                    eprintln!("{}: {error}", script_path.display());
                    std::process::exit(1);
                }
            });
        }
    });

    Ok(())
}

fn create_and_execute_script(script_path: &Path) -> anyhow::Result<()> {
    create_script(script_path)?;
    execute_script(script_path)?;
    Ok(())
}

fn create_script(script_path: &Path) -> anyhow::Result<()> {
    let mut file = std::fs::File::create(script_path).context("File creation failed")?;
    let mut permissions = file
        .metadata()
        .context("Failed to get file metadata")?
        .permissions();
    permissions.set_mode(0o700);
    file.set_permissions(permissions)
        .context("Set permissions failed")?;
    file.write_all(b"#!/bin/bash").context("Write failed")?;
    Ok(())
}

fn execute_script(script_path: &Path) -> anyhow::Result<()> {
    std::process::Command::new(script_path).status()?;
    Ok(())
}
