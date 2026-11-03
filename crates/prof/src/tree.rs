//! The per-thread call tree behind `span` (feature `enabled`); empty otherwise.

/// One node of the tree drained by [`drain`]: a span name under its callers.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Names from the outermost span to this one, joined by " > ".
    pub path: String,
    pub calls: u64,
    /// Nanoseconds including nested spans.
    pub incl_ns: u64,
    /// Nanoseconds without nested spans.
    pub excl_ns: u64,
    /// Allocation calls and bytes made directly in the span (nested spans not counted).
    pub allocs: u64,
    pub alloc_bytes: u64,
}

/// The calling thread's tree since the last drain (nodes that ran); counters restart.
#[cfg(not(feature = "enabled"))]
pub fn drain() -> Vec<Record> {
    Vec::new()
}

/// Switches measuring on or off at run time (spans entered while off are inert).
#[cfg(not(feature = "enabled"))]
pub fn set_active(_on: bool) {}

#[cfg(feature = "enabled")]
pub use on::{drain, enter, enter_debug, exit, set_active};

#[cfg(feature = "enabled")]
mod on {
    use super::Record;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::fmt::Write;
    use std::sync::atomic::{AtomicBool, Ordering};

    static ACTIVE: AtomicBool = AtomicBool::new(true);

    pub fn set_active(on: bool) {
        ACTIVE.store(on, Ordering::Relaxed);
    }

    struct Node {
        name: &'static str,
        parent: u32,
        children: Vec<u32>,
        calls: u64,
        incl: u64,
        excl: u64,
        allocs: u64,
        bytes: u64,
    }

    struct Frame {
        node: u32,
        start: u64,
        child_ns: u64,
        alloc0: (u64, u64),
        child_alloc: (u64, u64),
    }

    struct Tls {
        nodes: Vec<Node>,
        stack: Vec<Frame>,
        names: HashMap<String, &'static str>,
        text: String,
    }

    impl Tls {
        fn new() -> Self {
            let root = Node {
                name: "",
                parent: 0,
                children: Vec::new(),
                calls: 0,
                incl: 0,
                excl: 0,
                allocs: 0,
                bytes: 0,
            };
            Self {
                nodes: vec![root],
                stack: Vec::with_capacity(64),
                names: HashMap::new(),
                text: String::new(),
            }
        }

        fn child(&mut self, parent: u32, name: &'static str) -> u32 {
            let found = self.nodes[parent as usize]
                .children
                .iter()
                .copied()
                .find(|&c| self.nodes[c as usize].name == name);
            if let Some(found) = found {
                return found;
            }
            let id = self.nodes.len() as u32;
            self.nodes.push(Node {
                name,
                parent,
                children: Vec::new(),
                calls: 0,
                incl: 0,
                excl: 0,
                allocs: 0,
                bytes: 0,
            });
            self.nodes[parent as usize].children.push(id);
            id
        }

        fn push(&mut self, name: &'static str) {
            let parent = self.stack.last().map_or(0, |f| f.node);
            let node = self.child(parent, name);
            self.stack.push(Frame {
                node,
                start: crate::now_ns(),
                child_ns: 0,
                alloc0: crate::alloc::thread_totals(),
                child_alloc: (0, 0),
            });
        }
    }

    thread_local! {
        static TLS: RefCell<Tls> = RefCell::new(Tls::new());
    }

    #[inline]
    pub fn enter(name: &'static str) -> bool {
        if !ACTIVE.load(Ordering::Relaxed) {
            return false;
        }
        TLS.try_with(|t| t.borrow_mut().push(name)).is_ok()
    }

    pub fn enter_debug(prefix: &'static str, what: &dyn std::fmt::Debug) -> bool {
        if !ACTIVE.load(Ordering::Relaxed) {
            return false;
        }
        TLS.try_with(|t| {
            let mut t = t.borrow_mut();
            let mut text = std::mem::take(&mut t.text);
            text.clear();
            text.push_str(prefix);
            let _ = write!(text, "{what:?}");
            let name = match t.names.get(text.as_str()) {
                Some(name) => *name,
                None => {
                    let leaked: &'static str = Box::leak(text.clone().into_boxed_str());
                    t.names.insert(text.clone(), leaked);
                    leaked
                }
            };
            t.text = text;
            t.push(name);
        })
        .is_ok()
    }

    #[inline]
    pub fn exit() {
        let end = crate::now_ns();
        let totals = crate::alloc::thread_totals();
        let _ = TLS.try_with(|t| {
            let mut t = t.borrow_mut();
            let Some(frame) = t.stack.pop() else {
                return;
            };
            let elapsed = end.saturating_sub(frame.start);
            let spent = (totals.0 - frame.alloc0.0, totals.1 - frame.alloc0.1);
            let node = &mut t.nodes[frame.node as usize];
            node.calls += 1;
            node.incl += elapsed;
            node.excl += elapsed.saturating_sub(frame.child_ns);
            node.allocs += spent.0.saturating_sub(frame.child_alloc.0);
            node.bytes += spent.1.saturating_sub(frame.child_alloc.1);
            if let Some(parent) = t.stack.last_mut() {
                parent.child_ns += elapsed;
                parent.child_alloc.0 += spent.0;
                parent.child_alloc.1 += spent.1;
            }
        });
    }

    /// The calling thread's tree since the last drain (nodes that ran), counters reset.
    pub fn drain() -> Vec<Record> {
        TLS.try_with(|t| {
            let mut t = t.borrow_mut();
            let mut out = Vec::new();
            for id in 1..t.nodes.len() {
                if t.nodes[id].calls == 0 {
                    continue;
                }
                let mut names = Vec::new();
                let mut at = id as u32;
                while at != 0 {
                    names.push(t.nodes[at as usize].name);
                    at = t.nodes[at as usize].parent;
                }
                names.reverse();
                let n = &t.nodes[id];
                out.push(Record {
                    path: names.join(" > "),
                    calls: n.calls,
                    incl_ns: n.incl,
                    excl_ns: n.excl,
                    allocs: n.allocs,
                    alloc_bytes: n.bytes,
                });
            }
            for n in &mut t.nodes {
                n.calls = 0;
                n.incl = 0;
                n.excl = 0;
                n.allocs = 0;
                n.bytes = 0;
            }
            out
        })
        .unwrap_or_default()
    }
}
