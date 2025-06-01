use std::{io::Write, ptr::write_bytes};

pub mod shared;

struct Buffer {
    // buf is the actual data in the buffer
    buf: Box<[u8]>,
    // write_offset is where the next write will start
    write_offset: usize,
    // read_offset is where the next read will start
    read_offset: usize,
    // read_watermark is 0 if the buffer isn't inverted, and if the buffer is
    // inverted it indicates where the next read should end.
    read_watermark: usize,
}
impl Buffer {
    fn new(cap: usize) -> Self {
        Self {
            buf: vec![0; cap].into_boxed_slice(),
            write_offset: 0,
            read_offset: 0,
            read_watermark: 0,
        }
    }
    fn write(&mut self, p: &[u8]) -> bool {
        let sz = p.len();

        // inverted means that there is still data for the reader to read towards
        // the end of the buffer, but free space towards the beginning of the buffer
        // and we (the writer) are currently working on filling up that free space
        // towards the beginning of the buffer.
        let already_inverted = self.write_offset < self.read_offset;

        // we can write either up to the end of the buffer, or in the case of inversion
        // up to the start of the unread data in the buffer.
        let write_cap = if already_inverted {
            self.read_offset
        } else {
            self.buf.len()
        };

        let (start, end) = if self.write_offset + sz < write_cap {
            (self.write_offset, self.write_offset + sz)
        } else if !already_inverted && sz < self.read_offset {
            // Leave a readWatermark so the reader knows where the end of data in the buffer is.
            // We only set readWatermark when we're flipping from non-inverted -> inverted.
            self.read_watermark = self.write_offset;
            // We don't have space at the end of the buffer, but we have enough at the start!
            (0, sz)
        } else {
            // No space anywhere
            return false;
        };

        self.buf[start..end].copy_from_slice(p);
        self.write_offset = end;
        true
    }

    fn read<'a>(&'_ self) -> Option<Lease<'a>> {
        let start = self.read_offset;
        let end = if self.read_watermark > 0 {
            self.read_watermark
        } else {
            self.write_offset
        };
        if start == end {
            return None;
        }
        let ptr = self.buf.as_ptr();
        let view = unsafe { std::slice::from_raw_parts(ptr.add(start), end - start) };
        Some(Lease { view, end })
    }

    fn release(&mut self, l: Lease) {
        if l.end == self.write_offset {
            // Optimization: if we have caught up to the writer, reset everything
            self.read_offset = 0;
            self.write_offset = 0;
        } else if l.end == self.read_offset {
            // if the writer has already inverted and there is no more data to read
            // at the end of the buffer, move the reader to the start and clear the
            // read watermark
            self.read_offset = 0;
            self.read_watermark = 0;
        } else {
            self.read_offset = l.end;
        }
    }
}
struct Lease<'a> {
    view: &'a [u8],
    end: usize,
}

#[cfg(test)]
mod test {
    use crate::Buffer;

    #[test]
    fn ownership_smoke_test() {
        let mut b = Buffer::new(16);
        assert!(b.write(b"hello"));
        let lease = b.read().unwrap();
        assert_eq!(lease.view, b"hello");
        assert!(b.write(b"goodbye"));
        assert_eq!(lease.view, b"hello");
        b.release(lease);
        let lease = b.read().unwrap();
        assert_eq!(lease.view, b"goodbye");
    }
}
