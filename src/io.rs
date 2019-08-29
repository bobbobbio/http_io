//! This module provides re-implementations of things from std::io for building without std

pub use crate::error::{Error, Result};
use core::cmp;

pub trait Read {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
    fn take(self, limit: u64) -> Take<Self>
    where
        Self: Sized,
    {
        Take {
            inner: self,
            limit: limit,
        }
    }
    fn bytes(self) -> Bytes<Self>
    where
        Self: Sized,
    {
        Bytes { inner: self }
    }

    fn read_exact(&mut self, mut buf: &mut [u8]) -> Result<()> {
        while !buf.is_empty() {
            match self.read(buf) {
                Ok(0) => break,
                Ok(n) => {
                    let tmp = buf;
                    buf = &mut tmp[n..];
                }
                Err(e) => return Err(e),
            }
        }
        if !buf.is_empty() {
            Err(Error::UnexpectedEof("failed to fill whole buffer".into()))
        } else {
            Ok(())
        }
    }
}

impl<T: Read + ?Sized> Read for &mut T {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        (**self).read(buf)
    }
}

impl<T: Read + ?Sized> Read for alloc::boxed::Box<T> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        (**self).read(buf)
    }
}

pub trait Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize>;
    fn flush(&mut self) -> Result<()>;

    fn write_all(&mut self, mut buf: &[u8]) -> Result<()> {
        while !buf.is_empty() {
            match self.write(buf) {
                Ok(0) => return Err(Error::UnexpectedEof("failed to write whole buffer".into())),
                Ok(n) => buf = &buf[n..],
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn write_fmt(&mut self, fmt: alloc::fmt::Arguments<'_>) -> Result<()> {
        struct Adaptor<'a, T: ?Sized + 'a> {
            inner: &'a mut T,
            error: Result<()>,
        }

        impl<T: Write + ?Sized> alloc::fmt::Write for Adaptor<'_, T> {
            fn write_str(&mut self, s: &str) -> alloc::fmt::Result {
                match self.inner.write_all(s.as_bytes()) {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        self.error = Err(e);
                        Err(alloc::fmt::Error)
                    }
                }
            }
        }

        let mut output = Adaptor {
            inner: self,
            error: Ok(()),
        };
        match alloc::fmt::write(&mut output, fmt) {
            Ok(()) => Ok(()),
            Err(..) => {
                if output.error.is_err() {
                    output.error
                } else {
                    Err(Error::Other("formatter error".into()))
                }
            }
        }
    }
}

impl<T: Write + ?Sized> Write for &mut T {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        (**self).write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        (**self).flush()
    }
}

impl<T: Write + ?Sized> Write for alloc::boxed::Box<T> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        (**self).write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        (**self).flush()
    }
}

pub struct BufWriter<T> {
    inner: T,
}

impl<T> BufWriter<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    pub fn into_inner(self) -> Result<T> {
        Ok(self.inner)
    }
}

impl<T: Write> Write for BufWriter<T> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

pub struct BufReader<T> {
    inner: T,
}

impl<T> BufReader<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T: Read> Read for BufReader<T> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.inner.read(buf)
    }
}

const DEFAULT_BUF_SIZE: usize = 8 * 1024;

pub fn copy<R: ?Sized, W: ?Sized>(reader: &mut R, writer: &mut W) -> Result<u64>
where
    R: Read,
    W: Write,
{
    let mut buf = [0u8; DEFAULT_BUF_SIZE];
    let mut written = 0;
    loop {
        let len = match reader.read(&mut buf) {
            Ok(0) => return Ok(written),
            Ok(len) => len,
            Err(e) => return Err(e),
        };
        writer.write_all(&buf[..len])?;
        written += len as u64;
    }
}

pub struct Empty {}

impl Read for Empty {
    fn read(&mut self, _: &mut [u8]) -> Result<usize> {
        Ok(0)
    }
}

pub struct Take<T> {
    inner: T,
    limit: u64,
}

impl<T> Take<T> {
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T: Read> Read for Take<T> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if self.limit == 0 {
            return Ok(0);
        }

        let max = core::cmp::min(buf.len() as u64, self.limit) as usize;
        let n = self.inner.read(&mut buf[..max])?;
        self.limit -= n as u64;
        Ok(n)
    }
}

pub struct Bytes<T> {
    inner: T,
}

impl<R: Read> core::iter::Iterator for Bytes<R> {
    type Item = Result<u8>;

    fn next(&mut self) -> Option<Result<u8>> {
        let mut byte = 0;
        loop {
            return match self.inner.read(core::slice::from_mut(&mut byte)) {
                Ok(0) => None,
                Ok(..) => Some(Ok(byte)),
                Err(e) => Some(Err(e)),
            };
        }
    }
}

pub fn empty() -> Empty {
    Empty {}
}

pub struct Cursor<T> {
    inner: T,
    pos: u64,
}

impl<T> Cursor<T> {
    pub fn new(inner: T) -> Self {
        Self { pos: 0, inner }
    }
}

impl<T> Cursor<T>
where
    T: AsRef<[u8]>,
{
    fn fill_buf(&mut self) -> Result<&[u8]> {
        let amt = cmp::min(self.pos, self.inner.as_ref().len() as u64);
        Ok(&self.inner.as_ref()[(amt as usize)..])
    }
}

impl<T> Read for Cursor<T>
where
    T: AsRef<[u8]>,
{
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let n = Read::read(&mut self.fill_buf()?, buf)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Read for &[u8] {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let amt = cmp::min(buf.len(), self.len());
        let (a, b) = self.split_at(amt);

        if amt == 1 {
            buf[0] = a[0];
        } else {
            buf[..amt].copy_from_slice(a);
        }

        *self = b;
        Ok(amt)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        if buf.len() > self.len() {
            return Err(Error::UnexpectedEof("failed to fill whole buffer".into()));
        }
        let (a, b) = self.split_at(buf.len());

        if buf.len() == 1 {
            buf[0] = a[0];
        } else {
            buf.copy_from_slice(a);
        }

        *self = b;
        Ok(())
    }
}
