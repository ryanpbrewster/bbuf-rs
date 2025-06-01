use std::{cell::UnsafeCell, ops::Range};

pub(crate) struct Tracker {
    capacity: usize,
    // write_offset is where the next write will start
    write_offset: usize,
    // read_offset is where the next read will start
    read_offset: usize,
    // read_watermark is 0 if the buffer isn't inverted, and if the buffer is
    // inverted it indicates where the next read should end.
    read_watermark: usize,
}
impl Tracker {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            write_offset: 0,
            read_offset: 0,
            read_watermark: 0,
        }
    }
    pub fn write(&mut self, sz: usize) -> Option<WriteLease> {
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
            self.capacity
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
            return None;
        };

        return Some(WriteLease::new(start..end))
    }

    pub fn read(&mut self) -> Option<ReadLease> {
        let start = self.read_offset;
        let end = if self.read_watermark > 0 {
            self.read_watermark
        } else {
            self.write_offset
        };
        if start == end {
            return None;
        }
        Some(ReadLease::new(start..end))
    }

    pub fn commit(&mut self, w: WriteLease) {
        self.write_offset = w.start + w.len;
    }

    pub fn release(&mut self, r: ReadLease) {
        let end = r.start + r.len;
        if end == self.write_offset {
            // Optimization: if we have caught up to the writer, reset everything
            self.read_offset = 0;
            self.write_offset = 0;
        } else if end == self.read_offset {
            // if the writer has already inverted and there is no more data to read
            // at the end of the buffer, move the reader to the start and clear the
            // read watermark
            self.read_offset = 0;
            self.read_watermark = 0;
        } else {
            self.read_offset = end;
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct WriteLease{
    pub start: usize,
    pub len: usize,
}
impl WriteLease {
    fn new(range: Range<usize>) -> Self {
        Self {
            start: range.start,
            len: range.end - range.start,
        }
    }
}
#[derive(PartialEq, Eq, Debug)]
pub struct ReadLease {
    pub start: usize,
    pub len: usize,
}
impl ReadLease {
    fn new(range: Range<usize>) -> Self {
        Self {
            start: range.start,
            len: range.end - range.start,
        }
    }
}


#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn basic_write_then_read() {
        let mut t = Tracker::new(10);

        assert_eq!(t.read(), None);

        let w = t.write(4).unwrap();
        assert_eq!(w, WriteLease::new(0..4));
        t.commit(w);
        let r = t.read().unwrap();
        assert_eq!(r, ReadLease::new(0..4));
        t.release(r);

        assert_eq!(t.read(), None);
    }

    #[test]
    fn out_of_space() {
        let mut t = Tracker::new(10);

        assert_eq!(t.write(11), None);

        {
            let w = t.write(4).unwrap();
            assert_eq!(w, WriteLease::new(0..4));
            t.commit(w);
        }
        {
            let w = t.write(4).unwrap();
            assert_eq!(w, WriteLease::new(4..8));
            t.commit(w);
        }
        assert_eq!(t.write(4), None);
    }
}