//! Simple crate that allows easy usage of rotating logfiles by faking being a
//! single [`std::io::Write`] implementor

#![warn(
    missing_docs,
    missing_debug_implementations,
    missing_copy_implementations
)]

use std::borrow::Cow;
use std::fs;
use std::io::{self, prelude::*};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A specifier for how often we should rotate files
#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq)]
#[non_exhaustive]
pub enum RotationPeriod {
    /// Rotate every N line terminato bytes (0x0a, b'\n')
    Lines(usize),

    /// Rotate every N bytes successfully written
    ///
    /// This does not count bytes that are not written to the underlying file
    /// (when the given buffer's len does not match with the return value of
    /// [`io::Write::write`])
    ///
    /// [`io::Write::write`]: https://doc.rust-lang.org/std/io/trait.Write.html#tymethod.write
    Bytes(usize),

    /// Rotate every time N amount of time passes
    ///
    /// This is calculated on every write and is based on comparing two [`Instant::now`] return values
    ///
    /// [`Instant::now`]: https://doc.rust-lang.org/std/time/struct.Instant.html#method.now
    Interval(Duration),
}

mod rotation_tracker;
use rotation_tracker::RotationTracker;

/// As per the name, a rotating file
///
/// Handles being a fake file which will automagicaly rotate as bytes are written into it
#[derive(Debug)]
pub struct RotatingFile {
    name: Cow<'static, str>,
    directory: PathBuf,
    rotation_tracker: RotationTracker,
    max_files: usize,

    current_file: Option<fs::File>,
}

impl RotatingFile {
    /// Create a new rotating file with the given base name, in the given directory, rotating every given period and
    /// with a max of a given number of files
    pub fn new<Name, Directory>(
        name: Name,
        directory: Directory,
        rotate_every: RotationPeriod,
        max_files: usize,
    ) -> Self
    where
        Name: Into<Cow<'static, str>>,
        Directory: Into<PathBuf>,
    {
        Self {
            name: name.into(),
            directory: directory.into(),
            rotation_tracker: RotationTracker::from(rotate_every),
            max_files,
            current_file: None,
        }
    }

    fn should_rotate(&self) -> bool {
        // If we have no current file, it's probably best if we make one :p
        self.current_file.is_none() || self.rotation_tracker.should_rotate()
    }

    // To calculate a given path's index it must look like this:
    // NAME.N.log
    // and we return the N component
    fn logfile_index<P: AsRef<Path>>(&self, path: P) -> Option<usize> {
        let path = path.as_ref();
        let filename = path.file_stem()?.to_str()?;
        let extension = path.extension()?;
        if filename.starts_with(self.name.as_ref()) && extension == "log" {
            filename[self.name.len() + '.'.len_utf8()..].parse().ok()
        } else {
            None
        }
    }

    // Increment a log file's index component by one by moving it
    fn increment_index(&self, index: usize, path: PathBuf) -> io::Result<()> {
        debug_assert_eq!(self.logfile_index(&path), Some(index));
        fs::rename(path, self.make_filepath(index + 1))
    }

    fn make_filepath(&self, index: usize) -> PathBuf {
        self.directory.join(format!("{}.{}.log", self.name, index))
    }

    fn create_file(&self) -> io::Result<fs::File> {
        // Find out what the biggest logfile index in the directory is
        let max_index = itertools::process_results(fs::read_dir(&self.directory)?, |dir| {
            dir.into_iter()
                .filter_map(|entry| self.logfile_index(entry.path()))
                .max()
        })?;

        // Increment all the existing logs by one so that we can create one with index 0
        // Overwrite the oldest one if we would have more than necessary
        if let Some(max_index) = max_index {
            (0..max_index + if max_index >= self.max_files { 0 } else { 1 })
                .rev()
                .try_for_each(|index| self.increment_index(index, self.make_filepath(index)))?;
        }

        // Make sure we pass `create_new` so that nobody tries to be sneaky and place a file under us
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(self.make_filepath(0))
    }

    fn current_file(&mut self) -> io::Result<&mut fs::File> {
        if self.should_rotate() {
            self.current_file = Some(self.create_file()?);
            self.rotation_tracker.reset();
        }

        Ok(self
            .current_file
            .as_mut()
            .expect("should've been created before"))
    }
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let file = self.current_file()?;
        let written = file.write(buf)?;
        self.rotation_tracker.wrote(&buf[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.current_file()?.flush()
    }
}
