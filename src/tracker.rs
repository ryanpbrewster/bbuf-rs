use std::ops::Range;

pub(crate) struct Tracker {
    capacity: usize,
    // write_offset is where the next write will start
    write_offset: usize,
    // read_offset is where the next read will start
    read_offset: usize,
    // inverted_at is 0 if the buffer isn't inverted, and if the buffer is
    // inverted it indicates where the last write ended (i.e., where the next
    // read should end).
    inverted_at: usize,
}
impl Tracker {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            write_offset: 0,
            read_offset: 0,
            inverted_at: 0,
        }
    }
    pub fn write(&mut self, sz: usize) -> Option<WriteLease> {
        // inverted means that there is still data for the reader to read towards
        // the end of the buffer, but free space towards the beginning of the buffer
        // and we (the writer) are currently working on filling up that free space
        // towards the beginning of the buffer.
        let already_inverted = self.inverted_at > 0;

        // we can write either up to the end of the buffer, or in the case of inversion
        // up to the start of the unread data in the buffer.
        let write_cap = if already_inverted {
            self.read_offset
        } else {
            self.capacity
        };

        let start = if self.write_offset + sz <= write_cap {
            // Simple case: there's enough space contiguous with our current cursor.
            self.write_offset
        } else if !already_inverted && sz <= self.read_offset {
            // Complex case: we don't have space at our current cursor, but if
            // we invert then we'll have enough space at the start of the
            // buffer!

            // Leave an inverted_at marker so the reader knows where the end of
            // data in the buffer is. We only set inverted_at when we're
            // flipping from normal -> inverted.
            self.inverted_at = self.write_offset;
            0
        } else {
            // No space anywhere
            return None;
        };

        return Some(WriteLease::new(start..start + sz));
    }

    pub fn read(&mut self) -> Option<ReadLease> {
        let start = self.read_offset;
        let end = if self.inverted_at > 0 {
            self.inverted_at
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
        } else if end == self.inverted_at {
            // if the writer has already inverted and there is no more data to read
            // at the end of the buffer, move the reader to the start and clear the
            // inversion marker.
            self.read_offset = 0;
            self.inverted_at = 0;
        } else {
            self.read_offset = end;
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct WriteLease {
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

    #[test]
    fn wraparound() {
        // We are trying to set things up so that we have data split
        // due to a wraparound, specifically data at 5..9 + 0..4

        let mut t = Tracker::new(10);
        // First, write 0..5
        {
            let w = t.write(5).unwrap();
            assert_eq!(w, WriteLease::new(0..5));
            t.commit(w);
        }
        // This bit requires interleaving reads and writes
        // in order to ensure that there is room open at the beginning.
        {
            let r = t.read().unwrap();
            assert_eq!(r, ReadLease::new(0..5));
            let w = t.write(4).unwrap();
            assert_eq!(w, WriteLease::new(5..9));
            t.commit(w);
            t.release(r);
        }
        // Now finish writing 0..4
        {
            let w = t.write(4).unwrap();
            assert_eq!(w, WriteLease::new(0..4));
            t.commit(w);
        }
        // Now we should be able to read 5..9, then 0..4
        {
            let r = t.read().unwrap();
            assert_eq!(r, ReadLease::new(5..9));
            t.release(r);
        }
        {
            let r = t.read().unwrap();
            assert_eq!(r, ReadLease::new(0..4));
            t.release(r);
        }
        assert_eq!(t.read(), None);
    }

    #[test]
    fn long_write() {
        let mut t = Tracker::new(10);
        let w = t.write(10).unwrap();
        assert_eq!(w, WriteLease::new(0..10));
        t.commit(w);
        let r = t.read().unwrap();
        assert_eq!(r, ReadLease::new(0..10));
        t.release(r);
    }

    #[test]
    fn long_wraparound_write() {
        let mut t = Tracker::new(10);
        {
            let w = t.write(5).unwrap();
            assert_eq!(w, WriteLease::new(0..5));
            t.commit(w);
        }
        {
            let r = t.read().unwrap();
            assert_eq!(r, ReadLease::new(0..5));
            let w = t.write(5).unwrap();
            assert_eq!(w, WriteLease::new(5..10));
            t.commit(w);
            t.release(r);
        }
        {
            let w = t.write(5).unwrap();
            assert_eq!(w, WriteLease::new(0..5));
            t.commit(w);
        }
        // Now the buffer is entirely full, with data from 5..10 + 0..5
        assert_eq!(t.write(1), None);
        {
            let r = t.read().unwrap();
            assert_eq!(r, ReadLease::new(5..10));
            t.release(r);
        }
        {
            let r = t.read().unwrap();
            assert_eq!(r, ReadLease::new(0..5));
            t.release(r);
        }
    }
}
