//! Simple crate that allows easy usage of rotating logfiles by faking being a
//! single [`std::io::Write`] implementor
//!
//! # Alright, sure, but what's a rotating logfile?
//!
//! Well, imagine we are logging *a lot*, and after a while we use up all our disk space with logs.
//! We don't want this, nobody wants this, so how do we solve it?
//!
//! We'll introduce the concept of changing what file we log to periodically, or in other words, we'll
//! *rotate* our log files so that we don't generate too much stored logging.
//!
//! One of the concepts that is involved in rotation is a limit to how many log files can exist at once.
//!
//! # Examples
//!
//! To demostrate what was said above, here's to create a file which rotates every day, storing up to a week of logs
//! in `/logs`
//!
//! ```rust
//! # use std::{time::Duration, num::NonZeroUsize};
//! # use file_rotator::{RotationPeriod, RotatingFile};
//! RotatingFile::new(
//!     "loggylog",
//!     "/logs",
//!     RotationPeriod::Interval(Duration::from_secs(60 * 60 * 24)),
//!     NonZeroUsize::new(7).unwrap(),
//! );
//! ```

#![warn(
    missing_docs,
    missing_debug_implementations,
    missing_copy_implementations
)]

use std::borrow::Cow;
use std::fs;
use std::io::{self, prelude::*};
use std::num::NonZeroUsize;
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

    /// Rotate only via [`RotatingFile::rotate`]
    ///
    /// [`RotatingFile::rotate']: struct.RotatingFile.html#method.rotate
    Manual,
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
    max_index: usize,

    current_file: Option<fs::File>,
}

impl RotatingFile {
    /// Create a new rotating file with the given base name, in the given directory, rotating every given period and
    /// with a max of a given number of files
    pub fn new<Name, Directory>(
        name: Name,
        directory: Directory,
        rotate_every: RotationPeriod,
        max_files: NonZeroUsize,
    ) -> Self
    where
        Name: Into<Cow<'static, str>>,
        Directory: Into<PathBuf>,
    {
        Self {
            name: name.into(),
            directory: directory.into(),
            rotation_tracker: RotationTracker::from(rotate_every),
            max_index: max_files.get() - 1,
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
        // Let's survey the directory and find out what's the biggest index in there
        let max_found_index = itertools::process_results(fs::read_dir(&self.directory)?, |dir| {
            dir.into_iter()
                .filter_map(|entry| self.logfile_index(entry.path()))
                .max()
        })?;

        // If we've found any logs, let's make sure we keep under `self.max_index`
        if let Some(mut max_found_index) = max_found_index {
            // First, let's check if we have the maximum amount of logs available (or maybe even more!)
            if max_found_index >= self.max_index {
                // If so, let's remove all of the ones >=self.max_index so that we can make room for one more
                (self.max_index..=max_found_index)
                    .try_for_each(|index| fs::remove_file(self.make_filepath(index)))?;

                // We'll need to update our `max_found_index` to avoid trying to
                // move stuff that isn't there, but we'll use a saturating
                // subtraction to handle the case where self.max_index == 0
                // (only one logfile ever)
                max_found_index = self.max_index.saturating_sub(1);
            }

            // If there are any logfiles remaining
            if self.max_index != 0 {
                // Increment all the remaining log files' indices so that we have
                // room for a new one with index 0
                (0..=max_found_index)
                    .rev()
                    .try_for_each(|index| self.increment_index(index, self.make_filepath(index)))?;
            }
        }

        // Make sure we pass `create_new` so that nobody tries to be sneaky and
        // place a file under us
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(self.make_filepath(0))
    }

    fn current_file(&mut self) -> io::Result<&mut fs::File> {
        if self.should_rotate() {
            self.rotate()?;
        }

        Ok(self
            .current_file
            .as_mut()
            .expect("should've been created before"))
    }

    /// Manually rotate the file out
    ///
    /// This is the only way that a file whose `rotation_period` is [`RotationPeriod::Manual`] can rotate.
    ///
    /// [`RotationPeriod::Manual`]: enum.RotationPeriod.html#variant.Manual
    pub fn rotate(&mut self) -> io::Result<()> {
        self.current_file = Some(self.create_file()?);
        self.rotation_tracker.reset();
        Ok(())
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::num::NonZeroUsize;
    use std::path::Path;

    use proptest::prelude::*;

    use super::{RotatingFile, RotationPeriod};

    fn assert_contains_files<P: AsRef<Path>>(directory: P, num: usize) {
        let p = directory.as_ref();
        assert_eq!(
            fs::read_dir(p).unwrap().count(),
            num,
            "Directory {:?} did not contain {} file(s)",
            p,
            num
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 15,
            ..ProptestConfig::default()
        })]

        #[test]
        fn test_max_files(name in "[a-zA-Z_-]+", n in 1..25usize) {
            let directory = tempfile::tempdir().unwrap();

            let mut file = RotatingFile::new(
                name,
                directory.path().to_owned(),
                RotationPeriod::Manual,
                NonZeroUsize::new(n).unwrap(),
            );

            assert_contains_files(&directory, 0);
            for i in 0..n {
                file.rotate().unwrap();
                assert_contains_files(&directory, i+1);
            }

            for _ in 0..n {
                assert_contains_files(&directory, n);
                file.rotate().unwrap();
            }
        }
    }
}
