use crossbeam::channel::Sender;

use crate::buffer;

#[derive(Clone)]
pub struct Handle {
    writer: buffer::Writer,
    tx: Sender<()>,
}
impl Handle {
    pub fn write(&mut self, p: &[u8]) {
        if self.writer.try_write(p) {
            let _ = self.tx.try_send(());
        }
    }
}
pub fn spawn<'scope, 'env : 'scope, W>(scope: &'scope std::thread::Scope<'scope, 'env>, capacity: usize, mut inner: W) -> Handle 
    where W : std::io::Write + Send + 'env {
    let (mut reader, writer) = crate::buffer::create(capacity);
    let (tx, rx) = crossbeam::channel::bounded(1);
    scope.spawn(move || {
        while let Ok(()) = rx.recv() {
            while let Some(lease) = reader.read() {
                if let Err(_err) = inner.write_all(lease.view) {
                    // emit telemetry
                }
            }
        }
        // Once all the notifiers have dropped, we are guaranteed that no more data
        // can be buffered. There may be some existing data, so drain the buffer
        // and then exit.
        while let Some(lease) = reader.read() {
            if let Err(_err) = inner.write_all(lease.view) {
                // emit telemetry
            }
        }
        let _ = inner.flush();
    });

    Handle { writer, tx, }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn smoke() {
        let mut buf = Vec::new();
        std::thread::scope(|scope| {
            let mut h = spawn(scope, 100, &mut buf);
            h.write(b"asdf");
            h.write(b"pqrs");
        });
        assert_eq!(buf, b"asdfpqrs");
    }
}