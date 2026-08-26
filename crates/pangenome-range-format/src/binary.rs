use std::io;

pub(crate) struct BinaryReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> BinaryReader<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub(crate) fn take(&mut self, length: usize) -> io::Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| invalid_data("unexpected end of binary data"))?;
        let result = &self.bytes[self.position..end];
        self.position = end;
        Ok(result)
    }

    pub(crate) fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("fixed integer slice"),
        ))
    }

    pub(crate) fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("fixed integer slice"),
        ))
    }

    pub(crate) fn bytes(&mut self) -> io::Result<Vec<u8>> {
        let length = u64_to_usize(self.u64()?)?;
        Ok(self.take(length)?.to_vec())
    }

    pub(crate) fn string(&mut self) -> io::Result<String> {
        String::from_utf8(self.bytes()?).map_err(|error| invalid_data(error.to_string()))
    }

    pub(crate) fn finish(self) -> io::Result<()> {
        if self.position == self.bytes.len() {
            Ok(())
        } else {
            Err(invalid_data(format!(
                "{} trailing bytes in binary data",
                self.bytes.len() - self.position
            )))
        }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }
}

pub(crate) fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_bytes(output: &mut Vec<u8>, bytes: &[u8]) -> io::Result<()> {
    put_u64(output, usize_to_u64(bytes.len())?);
    output.extend_from_slice(bytes);
    Ok(())
}

pub(crate) fn put_string(output: &mut Vec<u8>, value: &str) -> io::Result<()> {
    put_bytes(output, value.as_bytes())
}

pub(crate) fn usize_to_u64(value: usize) -> io::Result<u64> {
    u64::try_from(value).map_err(|_| invalid_data("usize does not fit in u64"))
}

pub(crate) fn usize_to_u32(value: usize) -> io::Result<u32> {
    u32::try_from(value).map_err(|_| invalid_data("usize does not fit in u32"))
}

pub(crate) fn u32_to_usize(value: u32) -> io::Result<usize> {
    usize::try_from(value).map_err(|_| invalid_data("u32 does not fit in usize"))
}

pub(crate) fn u64_to_usize(value: u64) -> io::Result<usize> {
    usize::try_from(value).map_err(|_| invalid_data("u64 does not fit in usize"))
}

pub(crate) fn count_bounded_by_bytes(
    count: u64,
    remaining: usize,
    minimum_bytes: usize,
    section: &str,
) -> io::Result<usize> {
    let count = u64_to_usize(count)?;
    if count > remaining / minimum_bytes {
        return Err(invalid_data(format!(
            "{section} count exceeds the remaining payload"
        )));
    }
    Ok(count)
}

pub(crate) fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
