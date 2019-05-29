//! This crate provides re-implementations of things from std::io for building without std

pub use crate::error::{Error, Result};

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
    let mut buf = unsafe {
        let mut buf: [u8; DEFAULT_BUF_SIZE] = core::mem::uninitialized();
        core::ptr::write_bytes(buf.as_mut_ptr(), 0, buf.len());
        buf
    };

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
