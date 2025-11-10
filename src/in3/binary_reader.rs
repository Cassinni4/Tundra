use std::io::{Read, Seek, SeekFrom};

pub struct BinaryReader<T: Read + Seek> {
    reader: T,
}

impl<T: Read + Seek> BinaryReader<T> {
    pub fn new(reader: T) -> Self {
        Self { reader }
    }

    pub fn seek(&mut self, pos: u64) -> std::io::Result<()> {
        self.reader.seek(SeekFrom::Start(pos))?;
        Ok(())
    }

    pub fn tell(&mut self) -> std::io::Result<u64> {
        self.reader.seek(SeekFrom::Current(0))
    }

    pub fn read_f32(&mut self) -> std::io::Result<f32> {
        let mut buf = [0u8; 4];
        self.reader.read_exact(&mut buf)?;
        Ok(f32::from_le_bytes(buf))
    }

    pub fn read_u16(&mut self) -> std::io::Result<u16> {
        let mut buf = [0u8; 2];
        self.reader.read_exact(&mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }

    pub fn read_u32(&mut self) -> std::io::Result<u32> {
        let mut buf = [0u8; 4];
        self.reader.read_exact(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    pub fn read_bytes(&mut self, count: usize) -> std::io::Result<Vec<u8>> {
        let mut buf = vec![0u8; count];
        self.reader.read_exact(&mut buf)?;
        Ok(buf)
    }

    pub fn read_f32_array(&mut self, count: usize) -> std::io::Result<Vec<f32>> {
        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            result.push(self.read_f32()?);
        }
        Ok(result)
    }

    pub fn read_u16_array(&mut self, count: usize) -> std::io::Result<Vec<u16>> {
        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            result.push(self.read_u16()?);
        }
        Ok(result)
    }
}