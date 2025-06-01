use std::{cell::UnsafeCell, sync::{Arc, Mutex, Weak}};

use crate::tracker::{ReadLease, Tracker, WriteLease};

pub struct Buffer {
    tracker: Mutex<Tracker>,
    data: UnsafeCell<Box<[u8]>>,
}

pub struct Reader(Arc<Buffer>);
pub struct Writer(Arc<Buffer>);

impl Buffer {
    fn new(capacity: usize) -> Self {
        Self {
            tracker: Mutex::new(Tracker::new(capacity)),
            data: UnsafeCell::new(vec![0; capacity].into_boxed_slice()),
        }
    }
    fn split(self) -> (Reader, Writer) {
        let b = Arc::new(self);
        (Reader(b.clone()), Writer(b))
    }
}
impl Writer {
    fn try_write(&self, p: &[u8]) -> bool {
        let mut guard = self.0.tracker.lock().unwrap();
        let Some(w) = guard.write(p.len()) else {
            return false
        };
        unsafe {
            let data = &mut *self.0.data.get();
            data[w.start..][..w.len].copy_from_slice(p);
        }
        guard.commit(w);
        true
    }
}
impl Reader {
    fn read(&mut self) -> Option<Lease> {
        let r = self.0.tracker.lock().unwrap().read()?;
        let view = unsafe {
            let data = &mut *self.0.data.get();
            &data[r.start..][..r.len]
        };
        Some(Lease { reader: self, lease: Some(r), view: view })
    }
}

pub struct Lease<'a> {
    reader: &'a mut Reader,
    lease: Option<ReadLease>,
    pub view: &'a [u8],
}
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let lease = self.lease.take().expect("lease must persist until Drop");
        self.reader.0.tracker.lock().unwrap().release(lease);
    }
}

#[cfg(test)]
mod test {
    use super::Buffer;

    #[test]
    fn smoke() {
        let (mut reader, writer) = Buffer::new(10).split();

        assert!(reader.read().is_none());

        assert!(writer.try_write(b"asdf"));
        assert!(writer.try_write(b"pqrs"));

        {
            let l = reader.read().unwrap();
            assert_eq!(l.view, b"asdfpqrs")
        }

        assert!(reader.read().is_none());
    }
}