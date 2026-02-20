use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

pub struct RingBuffer {
    inner: Mutex<Inner>,
    not_empty: Condvar,
    not_full: Condvar,
}

struct Inner {
    buffer: Vec<u8>,
    lengths: Vec<usize>,
    head: usize,
    tail: usize,
    len: usize,
    capacity: usize,
    max_packet_size: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize, max_packet_size: usize) -> Self {
        assert!(capacity > 0);
        assert!(max_packet_size > 0);

        Self {
            inner: Mutex::new(Inner {
                buffer: vec![0u8; capacity * max_packet_size],
                lengths: vec![0usize; capacity],
                head: 0,
                tail: 0,
                len: 0,
                capacity,
                max_packet_size,
            }),
            not_empty: Condvar::new(),
            not_full: Condvar::new(),
        }
    }

    pub fn push(&self, packet: &[u8]) {
        let mut inner = self.inner.lock().unwrap();
        assert!(packet.len() <= inner.max_packet_size);

        while inner.len == inner.capacity {
            inner = self.not_full.wait(inner).unwrap();
        }

        let tail = inner.tail;
        let offset = tail * inner.max_packet_size;

        inner.buffer[offset..offset + packet.len()].copy_from_slice(packet);

        inner.lengths[tail] = packet.len();

        inner.tail = (tail + 1) % inner.capacity;
        inner.len += 1;

        self.not_empty.notify_one();
    }

    /// Returns:
    /// - Some(packet_len) if a packet was read
    /// - None if timeout expired
    pub fn pop(&self, out_buf: &mut [u8], timeout: Option<Duration>) -> Option<usize> {
        let mut inner = self.inner.lock().unwrap();

        match timeout {
            None => {
                while inner.len == 0 {
                    inner = self.not_empty.wait(inner).unwrap();
                }
            }
            Some(total_timeout) => {
                let deadline = Instant::now() + total_timeout;

                while inner.len == 0 {
                    let now = Instant::now();
                    if now >= deadline {
                        return None;
                    }

                    let remaining = deadline - now;
                    let (guard, wait_result) =
                        self.not_empty.wait_timeout(inner, remaining).unwrap();

                    inner = guard;

                    if wait_result.timed_out() && inner.len == 0 {
                        return None;
                    }
                }
            }
        }

        let head = inner.head;
        let packet_len = inner.lengths[head];
        assert!(out_buf.len() >= packet_len);

        let offset = head * inner.max_packet_size;

        out_buf[..packet_len].copy_from_slice(&inner.buffer[offset..offset + packet_len]);

        inner.head = (head + 1) % inner.capacity;
        inner.len -= 1;

        self.not_full.notify_one();
        Some(packet_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn basic_push_pop() {
        let ringbuffer = RingBuffer::new(4, 1024);
        let data = b"hello world";

        ringbuffer.push(data);

        let mut buffer = vec![0u8; 1024];
        let len = ringbuffer.pop(&mut buffer, Some(Duration::from_millis(10)));

        assert_eq!(len, Some(data.len()));
        assert_eq!(&buffer[..data.len()], data);
    }

    #[test]
    fn wraparound_behavior() {
        let ringbuffer = RingBuffer::new(2, 128);

        ringbuffer.push(b"a");
        ringbuffer.push(b"b");

        let mut buffer = vec![0u8; 128];

        assert_eq!(ringbuffer.pop(&mut buffer, None), Some(1));
        assert_eq!(&buffer[..1], b"a");

        ringbuffer.push(b"c");

        assert_eq!(ringbuffer.pop(&mut buffer, None), Some(1));
        assert_eq!(&buffer[..1], b"b");

        assert_eq!(ringbuffer.pop(&mut buffer, None), Some(1));
        assert_eq!(&buffer[..1], b"c");
    }

    #[test]
    fn timeout_returns_none() {
        let ringbuffer = RingBuffer::new(4, 256);
        let mut buffer = vec![0u8; 256];

        let start = Instant::now();
        let result = ringbuffer.pop(&mut buffer, Some(Duration::from_millis(100)));
        let elapsed = start.elapsed();

        assert!(result.is_none());
        assert!(elapsed >= Duration::from_millis(90));
    }

    #[test]
    fn blocking_wakeup() {
        let ringbuffer = Arc::new(RingBuffer::new(4, 256));
        let ringbuffer_clone = ringbuffer.clone();

        let handle = thread::spawn(move || {
            let mut buffer = vec![0u8; 256];
            ringbuffer_clone.pop(&mut buffer, None)
        });

        thread::sleep(Duration::from_millis(50));
        ringbuffer.push(b"ping");

        let result = handle.join().unwrap();
        assert_eq!(result, Some(4));
    }

    #[test]
    fn concurrent_producer_consumer() {
        let ringbuffer = Arc::new(RingBuffer::new(8, 128));

        let producer = {
            let ringbuffer = ringbuffer.clone();
            thread::spawn(move || {
                for i in 0..1000 {
                    ringbuffer.push(&[i as u8]);
                }
            })
        };

        let consumer = {
            let ringbuffer = ringbuffer.clone();
            thread::spawn(move || {
                let mut buffer = vec![0u8; 128];
                for i in 0..1000 {
                    let len = ringbuffer.pop(&mut buffer, None).unwrap();
                    assert_eq!(len, 1);
                    assert_eq!(buffer[0], i as u8);
                }
            })
        };

        producer.join().unwrap();
        consumer.join().unwrap();
    }

    #[test]
    fn push_blocks_when_full() {
        let ringbuffer = Arc::new(RingBuffer::new(1, 64));
        ringbuffer.push(b"first");

        let ringbuffer_clone = ringbuffer.clone();

        let producer = thread::spawn(move || {
            ringbuffer_clone.push(b"second"); // should block until pop
        });

        thread::sleep(Duration::from_millis(50));

        let mut buffer = vec![0u8; 64];
        assert_eq!(ringbuffer.pop(&mut buffer, None), Some(5));
        assert_eq!(&buffer[..5], b"first");

        producer.join().unwrap();

        assert_eq!(ringbuffer.pop(&mut buffer, None), Some(6));
        assert_eq!(&buffer[..6], b"second");
    }

    #[test]
    fn timeout_enforced() {
        let ringbuffer = RingBuffer::new(1, 64);

        let mut buffer = [0u8; 64];
        assert_eq!(
            ringbuffer.pop(&mut buffer, Some(Duration::from_millis(50))),
            None
        );
    }

    #[test]
    fn max_packet_enforced() {
        let ringbuffer = RingBuffer::new(2, 4);

        ringbuffer.push(b"1234");

        let result = std::panic::catch_unwind(|| {
            ringbuffer.push(b"12345"); // too large
        });

        assert!(result.is_err());
    }
}
