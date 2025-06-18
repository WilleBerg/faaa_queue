use std::{ptr::null_mut, sync::atomic::{AtomicPtr as RawAtomicPtr, AtomicUsize, Ordering::SeqCst}};

use haphazard::{AtomicPtr as HpAtomicPtr, HazardPointer};
use crossbeam_utils::CachePadded;
use log::trace;

const BUFFER_SIZE: usize = 1024;

struct Node<T> {
    enqueue_index: CachePadded<AtomicUsize>,
    dequeue_index: CachePadded<AtomicUsize>,
    next: CachePadded<HpAtomicPtr<Node<T>>>,
    array: [RawAtomicPtr<T>; BUFFER_SIZE],
}

#[derive(Debug)]
pub struct FAAAQueue<T> {
    head: HpAtomicPtr<Node<T>>,
    tail: HpAtomicPtr<Node<T>>,
}

impl<T> Node<T> {
    fn new(data_ptr: *mut T) -> Self {
        let mut node = Self {
            enqueue_index: CachePadded::new(1.into()),
            dequeue_index: CachePadded::new(0.into()),
            next: unsafe { CachePadded::new(HpAtomicPtr::new(core::ptr::null_mut())) },
            array: [const { RawAtomicPtr::new(core::ptr::null_mut()) }; BUFFER_SIZE],
        };
        // NOTE: Copies the address.
        node.array[0] = RawAtomicPtr::new(data_ptr);
        node
    }

    fn empty() -> Self {
        Self {
            enqueue_index: CachePadded::new(0.into()),
            dequeue_index: CachePadded::new(0.into()),
            next: unsafe { CachePadded::new(HpAtomicPtr::new(core::ptr::null_mut())) },
            array: [const { RawAtomicPtr::new(core::ptr::null_mut()) }; BUFFER_SIZE],
        }
    }
}
impl<T> FAAAQueue<T> {
    fn enqueue(&self, item: T, hp: &mut HazardPointer) {
        let item_ptr = Box::into_raw(Box::new(item));
        loop {
            trace!("Loading tail now.");
            let ltail = self.tail.safe_load(hp).unwrap();
            let idx = ltail.enqueue_index.fetch_add(1, SeqCst);
            if idx > BUFFER_SIZE - 1 { // This node is full.
                trace!("Node is full");
                if ltail as *const _ != self.tail.load_ptr() {continue;}
                let lnext: *mut Node<T> = ltail.next.load_ptr();
                if lnext.is_null() {
                    // NOTE: Must copy item_ptr? Otherwise it would be moved
                    // out of scope?
                    let new_node = Box::into_raw(Box::new(Node::new(item_ptr)));
                    if unsafe { ltail.next.compare_exchange_ptr(null_mut(), new_node).is_ok() } {
                        let _ = unsafe { self.tail.compare_exchange_ptr(ltail as *const _ as *mut _, new_node) };
                        hp.reset_protection();
                        return;
                    }
                    // NOTE: Fine since it is dropping the pointer to item,
                    // which is a copy of item_ptr?
                    unsafe { drop(Box::from_raw(new_node)) };
                } else {
                    // Help other thread enqueue?
                    let _ = unsafe { self.tail.compare_exchange_ptr(ltail as *const _ as *mut _, lnext) };
                }
                continue;
            }
            trace!("Node not full");
            let item_null: *mut T = null_mut();
            trace!("Attempting cas to add item.");
            if ltail.array[idx].compare_exchange(item_null, item_ptr, SeqCst, SeqCst).is_ok() {
                trace!("Succeeded");
                hp.reset_protection();
                trace!("returning now");
                return;
            }
        }
    }
    fn dequeue(&self, hp: &mut HazardPointer) -> Option<T> {
        loop {
            let lhead = self.head.safe_load(hp).unwrap();
            if lhead.dequeue_index.load(SeqCst) >= lhead.enqueue_index.load(SeqCst)
                && lhead.next.load_ptr().is_null() { break; }
            let idx = lhead.dequeue_index.fetch_add(1, SeqCst);
            if idx > BUFFER_SIZE - 1 { // Node has been drained
                let lnext = lhead.next.load_ptr();
                if lnext.is_null() { break; }
                if let Ok(old_ptr) =  unsafe { self.head.compare_exchange_ptr(lhead as *const _ as *mut _, lnext) } {
                    unsafe { old_ptr.unwrap().retire(); } 
                }
                continue;
            }
            let item_ptr = lhead.array[idx].swap(1u64 as *mut u64 as *mut T, SeqCst);
            if item_ptr.is_null() {continue;}
            let item = *unsafe { Box::from_raw(item_ptr) };
            return Some(item);
        }
        hp.reset_protection();
        None
    }
    fn new() -> Self {
        let start_node = Box::into_raw(Box::new(Node::empty()));
        Self {
            head: unsafe { HpAtomicPtr::new(start_node) },
            tail: unsafe { HpAtomicPtr::new(start_node) },
        }
    }
}
impl<T> Drop for FAAAQueue<T> {
    fn drop(&mut self) {
        trace!("Starting drop FAAArrayQueue");
        let head: Box<Node<T>> = unsafe { Box::from_raw(self.head.load_ptr()) };
        let mut next = head.next;

        while !next.load_ptr().is_null() {
            let node: Box<Node<T>> = unsafe { Box::from_raw(next.load_ptr()) };
            for data in node.array
            {
                let reclaimed_mem = data.load(SeqCst);
                if !reclaimed_mem.is_null() {
                    unsafe { drop(Box::from_raw(data.load(SeqCst))) };
                }
            }

            next = node.next;
        }
        trace!("Done dropping");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicI32;


    use log::info;

    use super::*;

    #[test]
    fn create_faaaq_queue() {
        let _ = env_logger::builder().is_test(true).try_init();
        info!("Creating queue");
        let q: FAAAQueue<i32> = FAAAQueue::new();
        info!("Done creating queue");
        let mut hp = HazardPointer::new();
        info!("Enqueueing now");
        q.enqueue(1, &mut hp);
        info!("Enqueue done");
        assert_eq!(q.dequeue(&mut hp), Some(1));
    }
    #[test]
    fn test_almost_full() {
        let _ = env_logger::builder().is_test(true).try_init();
        let q: FAAAQueue<usize> = FAAAQueue::new();
        let mut hp = HazardPointer::new();
        for i in 0..BUFFER_SIZE{
            q.enqueue(i, &mut hp);
        }
        for i in 0..BUFFER_SIZE{
            assert_eq!(q.dequeue(&mut hp), Some(i));
        }
    }
    #[test]
    fn test_double_buf_size() {
        let _ = env_logger::builder().is_test(true).try_init();
        let q: FAAAQueue<usize> = FAAAQueue::new();
        let mut hp = HazardPointer::new();
        for i in 0..BUFFER_SIZE * 2{
            q.enqueue(i, &mut hp);
        }
        for i in 0..BUFFER_SIZE * 2{
            assert_eq!(q.dequeue(&mut hp), Some(i));
        }
    }
}
