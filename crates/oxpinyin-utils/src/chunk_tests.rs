//! `tests/include/test_memory_chunk.cpp:5-69` at libpinyin 074a2219.

use oxpinyin_data::chunk_format::chunk_checksum;
use oxpinyin_input::tool::memory_chunk as output;
use oxpinyin_input::{chunk, tool};

/// Safe storage with the pin's logical size and allocation extent. The Vec's
/// allocator capacity is deliberately never observed (`memory_chunk.h:95-133`).
#[derive(Default)]
pub(crate) struct Buffer {
    data: Vec<u8>,
    size: usize,
    managed: bool,
}

impl Buffer {
    pub(crate) fn borrowed(bytes: &[u8]) -> Self {
        Self {
            data: bytes.to_vec(),
            size: bytes.len(),
            managed: false,
        }
    }

    fn more(&mut self, extra: usize) {
        if extra == 0 {
            return;
        }
        if !self.managed {
            self.data.resize(self.size + extra, 0);
            self.data[self.size..].fill(0);
            self.managed = true;
        } else if extra > self.data.len() - self.size {
            let capacity = (self.data.len() * 2).max(self.size + extra);
            self.data.resize(capacity, 0);
            self.data[self.size..].fill(0);
        }
    }

    pub(crate) fn resize(&mut self, size: usize) {
        if size > self.size {
            self.more(size - self.size);
        }
        self.size = size;
    }

    pub(crate) fn set(&mut self, offset: usize, bytes: &[u8]) {
        let end = offset + bytes.len();
        if end > self.size {
            self.more(end - self.size);
        }
        self.data[offset..end].copy_from_slice(bytes);
        self.size = self.size.max(end);
    }

    fn insert(&mut self, offset: usize, bytes: &[u8]) {
        self.more(bytes.len());
        self.data
            .copy_within(offset..self.size, offset + bytes.len());
        self.data[offset..offset + bytes.len()].copy_from_slice(bytes);
        self.size += bytes.len();
    }

    fn remove(&mut self, offset: usize, length: usize) {
        self.data.copy_within(offset + length..self.size, offset);
        self.size -= length;
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.data[..self.size]
    }

    fn capacity(&self) -> usize {
        self.data.len()
    }

    fn integer(&self, offset: usize) -> i32 {
        i32::from_ne_bytes(
            self.bytes()[offset..offset + 4]
                .try_into()
                .unwrap_or_else(|_| tool::abort()),
        )
    }

    fn short(&self, offset: usize) -> i32 {
        i32::from(i16::from_ne_bytes(
            self.bytes()[offset..offset + 2]
                .try_into()
                .unwrap_or_else(|_| tool::abort()),
        ))
    }
}

pub(crate) fn check(value: bool) {
    if !value {
        tool::abort();
    }
}

/// Run the pin's unchanged sequence, including save, heap load and mmap.
pub fn main() {
    let mut buffer = Buffer::default();
    buffer.set(0, &12_i32.to_ne_bytes());
    check(buffer.bytes().len() == 4);
    output::integers(&[buffer.integer(0)]);
    output::capacity(buffer.capacity());

    buffer = Buffer::borrowed(&12_i32.to_ne_bytes());
    buffer.set(4, &5_i16.to_ne_bytes());
    check(buffer.bytes().len() == 6);
    output::capacity(buffer.capacity());
    output::integers(&[buffer.integer(0), buffer.short(4)]);
    buffer.set(6, &5_i16.to_ne_bytes());
    check(buffer.bytes().len() == 8);
    output::capacity(buffer.capacity());
    output::integers(&[buffer.integer(0), buffer.short(4), buffer.short(6)]);
    buffer.resize(10);
    buffer.set(0, &12_i32.to_ne_bytes());
    buffer.set(8, &3_i16.to_ne_bytes());
    output::integers(&[
        buffer.integer(0),
        buffer.short(4),
        buffer.short(6),
        buffer.short(8),
    ]);

    buffer = Buffer::borrowed(&10_i32.to_ne_bytes());
    buffer.insert(4, &12_i32.to_ne_bytes());
    buffer.insert(4, &11_i32.to_ne_bytes());
    output::integers(&[buffer.integer(0), buffer.integer(4), buffer.integer(8)]);
    buffer.remove(4, 4);
    output::integers(&[buffer.integer(0), buffer.integer(4)]);
    check(buffer.bytes().get(4..8).is_some());
    output::integers(&[buffer.integer(4)]);

    let path = crate::fixed_tmp::path("test.bin");
    let path = path.as_path();
    let payload = buffer.bytes();
    let mut framed = Vec::with_capacity(8 + payload.len());
    framed.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
    framed.extend_from_slice(&chunk_checksum(payload).to_ne_bytes());
    framed.extend_from_slice(payload);
    check(chunk::save(path, &framed).is_ok());
    let loaded = chunk::load(path).unwrap_or_else(|_| tool::abort());
    let loaded = loaded
        .finish(chunk_checksum)
        .unwrap_or_else(|| tool::abort());
    drop(loaded);
    // HAVE_MMAP is enabled on the Linux reference host. The boundary uses
    // the same native mmap/MapView operation on each supported platform.
    let mut source = chunk::MappingSource::open(path).unwrap_or_else(|_| tool::abort());
    let length = u32::from_ne_bytes(source.read_word().unwrap_or_else(|_| tool::abort()));
    let checksum = u32::from_ne_bytes(source.read_word().unwrap_or_else(|_| tool::abort()));
    check((source.file_size() - 8) as u32 == length);
    let pending = source.map().unwrap_or_else(|_| tool::abort());
    check(chunk_checksum(&pending.as_slice()[8..8 + length as usize]) == checksum);
    drop(pending.finish());
}
