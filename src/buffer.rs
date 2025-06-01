use std::{cell::UnsafeCell, sync::{Arc, Mutex}};

use crate::tracker::{ReadLease, Tracker};

struct Buffer {
    tracker: Mutex<Tracker>,
    data: UnsafeCell<Box<[u8]>>,
}
pub struct Reader(Arc<Buffer>);
unsafe impl Sync for Reader {}
unsafe impl Send for Reader {}
#[derive(Clone)]
pub struct Writer(Arc<Buffer>);
unsafe impl Sync for Writer {}
unsafe impl Send for Writer {}
pub fn create(capacity: usize) -> (Reader, Writer) {
    let b = Arc::new(Buffer {
        tracker: Mutex::new(Tracker::new(capacity)),
        data: UnsafeCell::new(vec![0; capacity].into_boxed_slice()),
    });
    (Reader(b.clone()), Writer(b))
}

impl Writer {
    pub fn try_write(&mut self, p: &[u8]) -> bool {
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
    pub fn read(&mut self) -> Option<Lease> {
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
    use super::create;

    #[test]
    fn smoke() {
        let (mut reader, mut writer) = create(10);

        assert!(reader.read().is_none());

        assert!(writer.try_write(b"asdf"));
        assert!(writer.try_write(b"pqrs"));

        {
            let l = reader.read().unwrap();
            assert_eq!(l.view, b"asdfpqrs")
        }

        assert!(reader.read().is_none());
    }

    #[test]
    fn write_during_read_lease() {
        let (mut reader, mut writer) = create(10);

        assert!(writer.try_write(b"asdf"));

        // The reader can see the first write, and is allowed
        // to hold the lease even while writers continue to append.
        let l = reader.read().unwrap();
        assert_eq!(l.view, b"asdf");
        assert!(writer.try_write(b"pqrs"));
        assert_eq!(l.view, b"asdf");

        // Subsequent reads are needed to pick up the concurrent writes.
        drop(l);
        let l = reader.read().unwrap();
        assert_eq!(l.view, b"pqrs");
    }

}