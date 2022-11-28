use std::marker::PhantomData;
use std::ptr;
use std::sync::atomic::Ordering;
use seize::{AtomicPtr, Guard};
use crate::reclaim::{Atomic, Shared};

#[derive(Clone)]
pub struct Entry<V> {
    pub(crate) p: Atomic<V>,
    pub(crate) expunged: Atomic<V>,
}


impl<V> Entry<V>

{
    pub(crate) fn new(e: Shared<V>) -> Self {
        Self {
            p: Atomic::from(e),
            expunged: Atomic::null(),
        }
    }
    pub fn remove<'g>(&'g self, guard: &'g Guard<'_>) -> Option<&'g V> {
        let item = self.p.load(Ordering::SeqCst, guard);
        if item.is_null() /*TODO || self.p == self.EXPUNGED*/ {
            return None;
        }
        if let Ok(v) = self.p.compare_exchange(item, Shared::null(), Ordering::AcqRel, Ordering::Acquire, guard) {
            if let Some(v) = unsafe { item.as_ref() } {
                let v = &**v;

                return Some(v);
            }
        }

        return None;
    }
    pub fn load<'g>(&'g self, guard: &'g Guard<'_>) -> Option<&'g V> {
        let item = self.p.load(Ordering::SeqCst, guard);
        if item.is_null() /*TODO || self.p == self.EXPUNGED*/ {
            return None;
        }
        if let Some(v) = unsafe { item.as_ref() } {
            let v = &**v;

            return Some(v);
        }
        return None;
    }

    //todo check later
    pub(crate) fn try_store<'g>(&'g self, value: Shared<V>, guard: &'g Guard<'_>) -> bool {
        loop {
            let load = self.p.load(Ordering::SeqCst, guard);

            if load == self.expunged.load(Ordering::SeqCst, guard) {
                return false;
            }
            if self.p.compare_exchange(load, value, Ordering::AcqRel, Ordering::Acquire, guard).is_ok() {
                return true;
            }
        }
    }

    pub fn unexpunge_locked<'g>(&'g self, guard: &'g Guard<'_>) -> bool {
        let exp = self.expunged.load(Ordering::SeqCst, guard);
        self.expunged.compare_exchange(exp, Shared::null(), Ordering::AcqRel, Ordering::Acquire, guard).is_ok()
    }

    pub fn try_unexpunge_locked<'g>(&'g self, guard: &'g Guard<'_>) -> bool {
        let mut p = self.p.load(Ordering::SeqCst, guard);
        while p.is_null() {
            if self.p.compare_exchange(p,self.expunged.load(Ordering::SeqCst,guard),Ordering::AcqRel,Ordering::Acquire,guard).is_ok() {
                return true;
            }
            p = self.p.load(Ordering::SeqCst, guard);
        }
        return p == self.expunged.load(Ordering::SeqCst,guard)
    }


    pub(crate) fn store_locked<'g>(&'g self, value: Shared<V>, guard: &'g Guard<'_>) {

        self.p.swap(value, Ordering::SeqCst,guard);
    }
}


impl<V> Drop for Entry<V> {
    fn drop(&mut self) {
        println!("drop entry ")
    }
}

