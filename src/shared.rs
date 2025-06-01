use std::cell::UnsafeCell;
use std::sync::{Arc, Mutex};

/// Creates a new shared buffer with the specified capacity and returns a (Reader, Writer) pair.
pub fn new_shared_buffer(capacity: usize) -> (Reader, Writer) {
    let inner = Arc::new(Mutex::new(SharedBuffer {
        data: UnsafeCell::new(vec![0; capacity].into_boxed_slice()),
        read_offset: 0,
        write_offset: 0,
        capacity,
    }));

    let reader = Reader {
        inner: inner.clone(),
    };
    let writer = Writer { inner };

    (reader, writer)
}

struct SharedBuffer {
    data: UnsafeCell<Box<[u8]>>,
    // capacity is how many bytes `data` can hold; it's slightly convenient to have it
    // here rather than having to reach into the UnsafeCell to grab it.
    capacity: usize,
    // write_offset is where the next write will start
    write_offset: usize,
    // read_offset is where the next read will start
    read_offset: usize,
    // read_watermark is 0 if the buffer isn't inverted, and if the buffer is
    // inverted it indicates where the next read should end.
    read_watermark: usize,
}

impl SharedBuffer {
    unsafe fn view(&self, from: usize, len: usize) -> &[u8] {
        unsafe {
            let data = &*self.data.get();
            std::slice::from_raw_parts(data.as_ptr().add(from), len)
        }
    }
    unsafe fn view_mut(&self, from: usize, len: usize) -> &mut [u8] {
        unsafe {
            let data = &mut *self.data.get();
            std::slice::from_raw_parts_mut(data.as_mut_ptr().add(from), len)
        }
    }

    pub fn read(&self) -> Option<Lease<'_>> {
        let (end, view) = {
            let inner = self.inner.lock().unwrap();
            let end = if inner.read_watermark > 0 {
                inner.read_watermark
            } else {
                inner.write_offset
            };
            let len = end - inner.read_offset;
            if len == 0 {
                return None;
            }
            let view = unsafe { inner.view(inner.read_offset, len) };
            (end, view)
        };
        Some(Lease {
            view,
            end,
            buffer: &self.inner,
        })
    }
}

#[derive(Clone)]
pub struct Writer {
    inner: Arc<Mutex<SharedBuffer>>,
}

impl Writer {
    pub fn append(&mut self, p: &[u8]) -> bool {
        let sz = p.len();
        if sz == 0 {
            return true;
        }

        let mut inner = self.inner.lock().unwrap();
        // inverted means that there is still data for the reader to read towards
        // the end of the buffer, but free space towards the beginning of the buffer
        // and we (the writer) are currently working on filling up that free space
        // towards the beginning of the buffer.
        let already_inverted = inner.write_offset < inner.read_offset;

        // we can write either up to the end of the buffer, or in the case of inversion
        // up to the start of the unread data in the buffer.
        let write_cap = if already_inverted {
            inner.read_offset
        } else {
            inner.capacity
        };

        let start = if inner.write_offset + sz < write_cap {
            inner.write_offset
        } else if !already_inverted && sz < inner.read_offset {
            // Leave a readWatermark so the reader knows where the end of data in the buffer is.
            // We only set readWatermark when we're flipping from non-inverted -> inverted.
            inner.read_watermark = inner.write_offset;
            // We don't have space at the end of the buffer, but we have enough at the start!
            0
        } else {
            // No space anywhere
            return false;
        };
        let dst = unsafe { inner.view_mut(start, sz) };
        dst.copy_from_slice(p);
        inner.write_offset += sz;
        true
    }
}

pub struct Reader {
    inner: Arc<Mutex<SharedBuffer>>,
}

impl Reader {
}

pub struct Lease<'a> {
    view: &'a [u8],
    end: usize,
    buffer: &'a Mutex<SharedBuffer>,
}

impl<'a> Drop for Lease<'a> {
    fn drop(&mut self) {
        let mut inner = self.buffer.lock().unwrap();
        inner.read_offset = self.end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_read() {
        let (mut reader, mut writer) = new_shared_buffer(1024);

        // Initially empty
        {
            let lease = reader.read();
            assert_eq!(lease.len(), 0);
        } // lease dropped here, read_pos advances by 0

        // Write some data
        writer.append(1);
        writer.append(2);
        writer.append(3);

        // First read gets all data
        {
            let lease = reader.read();
            assert_eq!(&*lease, &[1, 2, 3]);
        } // lease dropped, read_pos advances by 3

        // Second read gets nothing (no new data)
        {
            let lease = reader.read();
            assert_eq!(lease.len(), 0);
        }

        // Write more data
        writer.append(4);
        writer.append(5);

        // Third read gets only new data
        {
            let lease = reader.read();
            assert_eq!(&*lease, &[4, 5]);
        } // lease dropped, read_pos advances by 2

        // Fourth read gets nothing again
        {
            let lease = reader.read();
            assert_eq!(lease.len(), 0);
        }
    }

    #[test]
    fn test_lease_deref() {
        let (mut reader, mut writer) = new_shared_buffer(1024);

        writer.append(10);
        writer.append(20);

        let lease = reader.read();
        assert_eq!(lease.len(), 2);
        assert_eq!(lease[0], 10);
        assert_eq!(lease[1], 20);

        // Can use slice methods
        assert!(lease.contains(&10));
        assert_eq!(lease.iter().sum::<u8>(), 30);
    }

    #[test]
    fn test_multiple_writers_streaming() {
        let (mut reader, mut writer) = new_shared_buffer(1024);
        let mut writer2 = writer.clone();

        writer.append(1);
        writer2.append(2);

        {
            let lease = reader.read();
            assert_eq!(lease.len(), 2);
        }

        writer.append(3);

        {
            let lease = reader.read();
            assert_eq!(&*lease, &[3]);
        }
    }

    #[test]
    fn test_custom_capacity() {
        let (mut reader, mut writer) = new_shared_buffer(3);

        // Fill small buffer
        assert!(writer.append(1));
        assert!(writer.append(2));
        assert!(writer.append(3));

        // Should be full now
        assert!(!writer.append(4));

        let lease = reader.read();
        assert_eq!(&*lease, &[1, 2, 3]);
    }
}
