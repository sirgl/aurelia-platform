//! A backend for tests: records what the store asks of it, with a clock the test moves.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use super::store::{Millis, Op, Persist};

#[derive(Default)]
pub struct Log {
    pub now: Millis,
    pub durable_off: bool,
    /// How many times the store asked for a flush.
    pub scheduled: u32,
    /// Batches handed over, in order.
    pub batches: Vec<(u64, Vec<Op>)>,
}

#[derive(Clone, Default)]
pub struct Fake(pub Rc<RefCell<Log>>);

impl Fake {
    pub fn at(now: Millis) -> Self {
        let fake = Self::default();
        fake.0.borrow_mut().now = now;
        fake
    }

    pub fn tick(&self, ms: Millis) {
        self.0.borrow_mut().now += ms;
    }

    pub fn batches(&self) -> Vec<(u64, Vec<Op>)> {
        self.0.borrow().batches.clone()
    }

    /// What a durable store would hold after every batch so far (the "disk" a page reload starts from).
    pub fn disk(&self) -> BTreeMap<String, (Vec<u8>, Millis)> {
        let mut disk = BTreeMap::new();
        for (_, batch) in &self.0.borrow().batches {
            for op in batch {
                match op {
                    Op::Put { key, bytes, modified } => {
                        disk.insert(key.clone(), (bytes.clone(), *modified));
                    }
                    Op::Delete { key } => {
                        disk.remove(key);
                    }
                }
            }
        }
        disk
    }
}

impl Persist for Fake {
    fn now_ms(&self) -> Millis {
        self.0.borrow().now
    }

    fn schedule_flush(&mut self) {
        self.0.borrow_mut().scheduled += 1;
    }

    fn submit(&mut self, id: u64, batch: Vec<Op>) {
        self.0.borrow_mut().batches.push((id, batch));
    }

    fn durable(&self) -> bool {
        !self.0.borrow().durable_off
    }
}
