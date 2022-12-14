use std::borrow::Borrow;
use std::collections::HashMap;
use std::fmt;
use std::fmt::{Debug, Formatter};
use std::hash::{BuildHasher, Hash, Hasher};
use std::ops::Deref;
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
use std::sync::Mutex;
use seize::{Collector, Guard};
use crate::entry::Entry;
use crate::reclaim::{Atomic, Shared};

macro_rules! load_factor {
    ($n: expr) => {
        // ¾ n = n - n/4 = n - (n >> 2)
        $n - ($n >> 2)
    };
}
///Map is like a  Hashmap but is safe for concurrent use by multiple thread without additional locking or coordination.
/// Loads, stores, and deletes run in amortized constant time.
///The Map type is specialized. Most code should use a plain Rust HashMap instead, with separate locking or coordination, f
/// or better type safety and to make it easier to maintain other invariants along with the map content.
///The Map type is optimized for two common use cases: (1) when the entry for a given key is
/// only ever written once but read many times, as in caches that only grow, or (2) when
/// multiple thread read, write, and overwrite entries for disjoint sets of keys. In these two cases,
/// use of a Map may significantly reduce lock contention compared to a Rust HashMap paired with a
/// separate Mutex
pub struct Map<K, V, S = crate::DefaultHashBuilder> {
    read: Atomic<ReadOnly<K, V>>,
    dirty: Atomic<HashMap<K, *mut Entry<V>>>,
    misses: AtomicUsize,
    flag_ctl: AtomicIsize,
    build_hasher: S,
    collector: Collector,
    lock: Mutex<()>,
}

impl<K, V, S> fmt::Debug for Map<K, V, S>
    where
        K: Debug,
        V: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let guard = self.collector.enter();
        f.debug_map().finish()
    }
}

impl<K, V, S> Clone for Map<K, V, S>
    where
        K: Sync + Send + Clone + Hash + Ord,
        V: Sync + Send + Clone,
        S: BuildHasher + Clone,
{
    fn clone(&self) -> Map<K, V, S> {
        let mut cloned_map = Map::with_hasher(self.build_hasher.clone());

        {
            cloned_map.dirty = self.dirty.clone();
            cloned_map.read = self.read.clone();
            cloned_map.misses = AtomicUsize::new(self.misses.load(Ordering::SeqCst));
            cloned_map.flag_ctl = AtomicIsize::new(self.flag_ctl.load(Ordering::SeqCst));

            // let dirty = self.dirty.load(Ordering::SeqCst, &guard);
            // if !dirty.is_null() {
            //     for (key, value) in unsafe { dirty.deref() }.deref() {
            //         let value = unsafe { (value.as_ref().unwrap()).p.load(Ordering::SeqCst, &guard).deref().deref() };
            //         cloned_map.insert(key.clone(), value.clone(), &cloned_guard)
            //     }
            // }
        }
        cloned_map
    }
}

impl<K, V> Map<K, V, crate::DefaultHashBuilder> {
    /// Creates an empty `HashMap`.
    ///
    /// The hash map is initially created with a capacity of 0, so it will not allocate until it
    /// is first inserted into.
    ///
    /// # Examples
    ///
    /// ```
    ///
    /// use syncmap::map::Map;
    /// let map: Map<&str, i32> = Map::new();
    /// ```
    pub fn new() -> Self {
        Self::default()
    }
}

impl<K, V, S> Default for Map<K, V, S>
    where
        S: Default,
{
    fn default() -> Self {
        Self::with_hasher(S::default())
    }
}

impl<K, V, S> Drop for Map<K, V, S> {
    fn drop(&mut self) {
        let guard = unsafe { Guard::unprotected() };

        // let read = self.read.swap(Shared::null(), Ordering::SeqCst, &guard);
        // assert!(
        //     !read.is_null(),
        //     "self.moved is initialized together with the table"
        // );
        //
        // // safety: we have mut access to self, so no-one else will drop this value under us.
        // let read = unsafe { read.into_box() };
        // drop(read);
         let read = self.read.swap(Shared::null(), Ordering::SeqCst, &guard);
        if !read.is_null() {
            let read = unsafe { read.into_box() };
            drop(read);
        }
        let moved = self.dirty.swap(Shared::null(), Ordering::SeqCst, &guard);
        if moved.is_null() {
            return;
        }
        assert!(
            !moved.is_null(),
            "self.moved is initialized together with the table"
        );

        // safety: we have mut access to self, so no-one else will drop this value under us.
        let moved = unsafe { moved.into_box() };
        drop(moved);
    }
}


impl<K, V, S> Map<K, V, S> {
    /// Creates an empty map which will use `hash_builder` to hash keys.
    ///
    /// The created map has the default initial capacity.
    ///
    /// Warning: `hash_builder` is normally randomly generated, and is designed to
    /// allow the map to be resistant to attacks that cause many collisions and
    /// very poor performance. Setting it manually using this
    /// function can expose a DoS attack vector.
    ///
    /// # Examples
    ///
    /// ```
    ///
    /// use syncmap::DefaultHashBuilder;
    /// use syncmap::map::Map;
    /// let map = Map::with_hasher(DefaultHashBuilder::default());
    ///   let guard = map.guard();
    /// map.insert(1, 2,&guard);
    /// ```
    pub fn with_hasher(hash_builder: S) -> Self {
        Self {
            read: Atomic::null(),
            dirty: Atomic::null(),
            misses: AtomicUsize::new(0),
            flag_ctl: AtomicIsize::new(0),
            build_hasher: hash_builder,
            collector: Collector::new(),
            lock: Mutex::new(()),
        }
    }

    /// Pin a `Guard` for use with this map.
    ///
    /// Keep in mind that for as long as you hold onto this `Guard`, you are preventing the
    /// collection of garbage generated by the map.
    pub fn guard(&self) -> Guard<'_> {
        self.collector.enter()
    }

    #[inline]
    fn check_guard(&self, guard: &Guard<'_>) {
        // guard.collector() may be `None` if it is unprotected
        if let Some(c) = guard.collector() {
            assert!(Collector::ptr_eq(c, &self.collector));
        }
    }

    fn init_table<'g>(&'g self, guard: &'g Guard<'_>) -> Shared<'g, ReadOnly<K, V>> {
        loop {
            let table = self.read.load(Ordering::SeqCst, guard);
            // safety: we loaded the ReadOnly while the thread was marked as active.
            // ReadOnly won't be deallocated until the guard is dropped at the earliest.
            if !table.is_null() {
                break table;
            }
            //try allocate ReadOnly
            let mut flag = self.flag_ctl.load(Ordering::SeqCst);
            if flag < 0 {
                //lost tje init race; just spin
                std::thread::yield_now();
                continue;
            }

            if self.flag_ctl
                .compare_exchange(flag, -1, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                let mut table = self.read.load(Ordering::SeqCst, guard);
                if table.is_null() {
                    let n = if flag > 0 {
                        flag as usize
                    } else {
                        1
                    };
                    table = Shared::boxed(ReadOnly::new(), &self.collector);
                    self.read.store(table, Ordering::SeqCst);
                    let m = Shared::boxed(HashMap::new(), &self.collector);
                    self.dirty.store(m, Ordering::SeqCst);
                    flag = load_factor!(n as isize)
                }
                self.flag_ctl.store(flag, Ordering::SeqCst);
                break table;
            }
        }
    }
}

impl<K, V, S> Map<K, V, S>
    where
        K: Clone + Hash + Ord,
        S: BuildHasher,
{
    #[inline]
    fn hash<Q: ?Sized + Hash>(&self, key: &Q) -> u64 {
        let mut h = self.build_hasher.build_hasher();
        key.hash(&mut h);
        h.finish()
    }

    /// Returns the number of entries in the map.
    ///
    /// # Examples
    ///
    /// ```
    /// use syncmap::map::Map;
    ///
    /// let map = Map::new();
    /// let guard = map.guard();
    /// map.insert(1, "a",&guard);
    /// map.insert(2, "b",&guard);
    /// assert!(map.len() == 2);
    /// ```
    pub fn len(&self) -> usize {
        let guard = self.guard();
        let map = self.dirty.load(Ordering::SeqCst, &guard);
        if map.is_null() {
            return 0;
        }
        unsafe { map.deref() }.len()
    }

    /// Returns a reference to the value corresponding to the key.
    ///
    /// The key may be any borrowed form of the map's key type, but
    /// [`Hash`] and [`Ord`] on the borrowed form *must* match those for
    /// the key type.
    ///
    /// [`Ord`]: std::cmp::Ord
    /// [`Hash`]: std::hash::Hash
    ///
    /// To obtain a `Guard`, use [`HashMap::guard`].
    ///
    /// # Examples
    ///
    /// ```
    ///
    /// use syncmap::map::Map;
    /// let map = Map::new();
    /// let guard = map.guard();
    /// map.insert(1,"a",&guard);
    /// assert_eq!(map.get(&1,&guard), Some(&"a"));
    /// assert_eq!(map.get(&2,&guard), None);
    /// ```
    #[inline]
    pub fn get<'g, Q>(&'g self, key: &Q, guard: &'g Guard<'_>) -> Option<&'g V>
        where
            K: Borrow<Q>,
            Q: ?Sized + Hash + Ord,
    {
        self.check_guard(guard);

        let read = self.read.load(Ordering::SeqCst, guard);
        if read.is_null() {
            return None;
        }
        let r = unsafe { read.deref() };
        let mut e = r.m.get(key);
        if e.is_none() && r.amended {
            let lock = self.lock.lock();
            let read = self.read.load(Ordering::SeqCst, guard);
            let r = unsafe { read.deref() };
            e = r.m.get(key);
            if e.is_none() && r.amended {
                let dirty = self.dirty.load(Ordering::SeqCst, guard);
                if dirty.is_null() {
                    drop(lock);
                    return None;
                }
                e = unsafe { dirty.deref() }.get(key);
                self.miss_locked(guard);
            }
            drop(lock)
        }
        if e.is_none() {
            return None;
        }

        /*  let v = unsafe { Box::from_raw(e.unwrap().as_mut().unwrap()) };
          let p = v.p.load(Ordering::SeqCst, &guard);
          if p.is_null() {
              return None;
          }
          if let Some(p) = unsafe {p.as_ref()} {
              let v = &**p;
              return Some(v)
          }*/
        unsafe { e.unwrap().as_ref().unwrap().load(guard) }
    }


    fn miss_locked<'g>(&'g self, guard: &'g Guard) {
        let miss = self.misses.fetch_add(1, Ordering::SeqCst);

        let dirty = self.dirty.load(Ordering::SeqCst, guard);
        if dirty.is_null() {
            return;
        }
        if miss < unsafe { dirty.deref() }.len() {
            return;
        }
        let mut map = HashMap::new();

        for (key, value) in unsafe { dirty.deref() }.deref() {
            map.insert(key.clone(), *value);
        }
        let read_only_map = Shared::boxed(ReadOnly {
            m: map,
            amended: false,
        }, &self.collector);
        self.read.store(read_only_map, Ordering::SeqCst);
        let old_map = self.dirty.load(Ordering::SeqCst, guard);
        if !old_map.is_null() {
            self.dirty.compare_exchange(old_map, Shared::null(), Ordering::AcqRel, Ordering::Acquire, guard);
        }
        self.misses.store(0, Ordering::SeqCst);
    }
}

impl<K, V, S> Map<K, V, S>
    where
        K: Sync + Send + Clone + Hash + Ord,
        V: Sync + Send,
        S: BuildHasher,
{
    /// Inserts a key-value pair into the map.
    ///
    /// If the map did not have this key present, [`None`] is returned.
    ///
    /// If the map did have this key present, the value is updated, and the old
    /// value is returned. The key is left unchanged. See the [std-collections
    /// documentation] for more.
    ///
    /// [`None`]: std::option::Option::None
    /// [std-collections documentation]: https://doc.rust-lang.org/std/collections/index.html#insert-and-complex-keys
    ///
    /// # Examples
    ///
    /// ```
    ///
    /// use syncmap::map::Map;
    /// let map = Map::new();
    /// let guard = map.guard();
    /// map.insert(1,1,&guard)
    pub fn insert<'g>(&'g self, key: K, value: V, guard: &'g Guard<'_>) {
        self.check_guard(guard);
        self.put(key, value, false, guard)
    }

    fn put<'g>(
        &'g self,
        mut key: K,
        value: V,
        no_replacement: bool,
        guard: &'g Guard<'_>,
    ) {
        let mut table = self.read.load(Ordering::SeqCst, guard);
        let entry_value = Shared::boxed(value, &self.collector);
        loop {
            if table.is_null() {
                table = self.init_table(guard);
                continue;
            }

            let read = unsafe { table.deref() };


            if let Some(v) = read.m.get(&key) {
                if unsafe { v.as_ref().unwrap() }.try_store(entry_value, guard) {
                    return;
                }
            }

            let lock = self.lock.lock();
            // TODO need to load readonlu again
            match read.m.get(&key) {
                Some(e) => {
                    if unsafe { e.as_ref().unwrap() }.unexpunge_locked(guard) {
                        // The entry was previously expunged, which implies that there is a
                        // non-nil dirty map and this entry is not in it.
                        let mut table = self.read.load(Ordering::SeqCst, guard);
                        unsafe {
                            let read = table.as_ptr();
                            let read = read.as_mut().unwrap();
                            read
                        }.m.insert(key.clone(), *e);
                    }
                    unsafe { e.as_ref().unwrap() }.store_locked(entry_value, guard);
                }
                None => {
                    let dirty = self.dirty.load(Ordering::SeqCst, guard);
                    if dirty.is_null() {
                        table = self.read.load(Ordering::SeqCst, guard);
                        let m = Shared::boxed(HashMap::new(), &self.collector);
                        self.dirty.store(m, Ordering::SeqCst);
                        drop(lock);
                        continue;
                    }
                    // TODO: check the dirty is null here
                    let d = unsafe { dirty.deref() };
                    if !d.is_empty() {
                        if let Some(e) = d.get(&key) {
                            unsafe { e.as_ref() }.unwrap().store_locked(entry_value, guard);
                            drop(lock);
                            break;
                        }
                    }

                    if !read.amended {
                        // We're adding the first new key to the dirty map.
                        // Make sure it is allocated and mark the read-only map as incomplete.
                        self.dirty_locked(key, entry_value, guard);
                        let shard = self.read.load(Ordering::SeqCst, guard);
                        let mut map = HashMap::new();
                        for (key, value) in &unsafe { shard.deref() }.m {
                            map.insert(key.clone(), *value);
                        }
                        let shard_map = Shared::boxed(ReadOnly {
                            m: map,
                            amended: true,
                        }, &self.collector);
                        self.read.store(shard_map, Ordering::SeqCst);
                        drop(lock);
                        break;
                    }
                    let dirty2 = self.dirty.load(Ordering::SeqCst, guard);
                    if dirty != dirty2 {
                        continue;
                    }
                    //save entry;
                    let mut entry = Entry {
                        p: Atomic::null(),
                        expunged: Atomic::null(),
                    };

                    entry.p.store(entry_value, Ordering::SeqCst);
                    unsafe {
                        let dirty2 = dirty2.as_ptr();
                        dirty2.as_mut().unwrap().insert(key.clone(), Box::into_raw(Box::new(entry)));
                    };
                }
            }

            drop(lock);
            break;
        }
    }


    /// Removes a key-value pair from the map, and returns the removed value (if any).
    ///
    /// The key may be any borrowed form of the map's key type, but
    /// [`Hash`] and [`Ord`] on the borrowed form *must* match those for
    /// the key type.
    ///
    /// [`Ord`]: std::cmp::Ord
    /// [`Hash`]: std::hash::Hash
    ///
    /// # Examples
    ///
    /// ```
    ///
    /// use syncmap::map::Map;
    /// let map = Map::new();
    /// map.insert(1, "a",&map.guard());
    /// assert_eq!(map.remove(&1,&map.guard()), Some(&"a"));
    /// assert_eq!(map.remove(&1,&map.guard()), None);
    /// ```
    pub fn remove<'g, Q>(&'g self, key: &Q, guard: &'g Guard<'_>) -> Option<&'g V>
        where
            K: Borrow<Q>,
            Q: ?Sized + Hash + Ord,
    {
        // NOTE: _technically_, this method shouldn't require the thread-safety bounds, but a) that
        // would require special-casing replace_node for when new_value.is_none(), and b) it's sort
        // of useless to call remove on a collection that you know you can never insert into.
        self.check_guard(guard);

        let mut read = self.read.load(Ordering::SeqCst, guard);
        loop {
            if read.is_null() {
                break None;
            }

            let r = unsafe { read.deref() };
            let mut remove_el: Option<*mut Entry<V>> = None;
            let mut e = r.m.get(&key);
            if e.is_none() && r.amended {
                let lock = self.lock.lock();

                e = r.m.get(&key);
                if e.is_none() && r.amended {
                    let dirty = self.dirty.load(Ordering::SeqCst, guard);
                    if dirty.is_null() {
                        read = self.read.load(Ordering::SeqCst, guard);
                        drop(lock);
                        continue;
                    }
                    e = unsafe { dirty.deref() }.get(&key);

                    let dirty = unsafe { dirty.as_ptr() };

                    remove_el = unsafe { dirty.as_mut().unwrap().remove(&key) };

                    self.miss_locked(guard);
                }
                drop(lock)
            } else {
                if let Some(e) = e {
                    return unsafe { e.as_mut().unwrap().remove(guard) };
                }
            }

            if remove_el.is_some() {
                let data = unsafe { remove_el.unwrap().as_mut().unwrap().remove(guard) };
                break data;
            }
            break None;
        }
    }

    fn dirty_locked<'g>(&'g self, key: K, entry_value: Shared<V>, guard: &Guard<'_>) {
        let dirty = self.dirty.load(Ordering::SeqCst, guard);
        if dirty.is_null() {
            return;
        }
        let read = self.read.load(Ordering::SeqCst, guard);
        let mut map = HashMap::with_capacity(unsafe { read.deref() }.m.len());
        for (key, value) in &unsafe { read.deref() }.m {
            if !unsafe { value.as_ref().unwrap() }.try_unexpunge_locked(guard) {
                map.insert(key.clone(), *value);
            }
        }
        let entry = Entry {
            p: Atomic::null(),
            expunged: Atomic::null(),
        };

        entry.p.store(entry_value, Ordering::SeqCst);

        map.insert(key, Box::into_raw(Box::new(entry)));
        self.dirty.store(Shared::boxed(map, &self.collector), Ordering::SeqCst)
    }
}

impl<K, V, S> Map<K, V, S>
    where
        K: Clone + Ord,
{
    /// Clears the map, removing all key-value pairs.
    ///
    /// # Examples
    ///
    /// ```
    ///
    /// use syncmap::map::Map;
    /// let map = Map::new();
    /// let guard = map.guard();
    /// map.insert(1, "a",&guard);
    /// map.clear(&guard);
    /// ```
    pub fn clear<'g>(&'g self, guard: &'g Guard<'_>) {
        let lock = self.lock.lock();

        self.dirty.store(Shared::boxed(HashMap::new(), &self.collector), Ordering::SeqCst);
        let read = self.read.load(Ordering::SeqCst, guard);
        self.read.store(Shared::boxed(ReadOnly::new(), &self.collector), Ordering::SeqCst);
        let sc = self.misses.load(Ordering::SeqCst);
        self.misses.compare_exchange(sc, 0, Ordering::AcqRel, Ordering::Acquire).expect("change miess");

        drop(lock);
    }
}


struct ReadOnly<K, V> {
    m: HashMap<K, *mut Entry<V>>,
    amended: bool,
}

impl<K, V> ReadOnly<K, V> {
    fn new() -> Self <> {
        Self {
            m: HashMap::new(),
            amended: false,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    use rayon;
    use rayon::prelude::*;

    use crate::reclaim::Shared;
    use super::*;

    const ITER: u64 = 32 * 1024;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn remove_and_insert() {
        let map = Arc::new(Map::<usize, usize>::new());
        let guard = map.guard();
        map.insert(1, 1, &guard);
        assert_eq!(map.remove(&1, &guard), Some(&1));
        assert_eq!(map.remove(&1, &guard), None)
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn concurrent_insert() {
        let map = Arc::new(Map::<usize, usize>::new());

        let map1 = map.clone();
        let t1 = std::thread::spawn(move || {
            for i in 0..5000 {
                map1.insert(i, 0, &map1.guard());
            }
        });
        let map2 = map.clone();
        let t2 = std::thread::spawn(move || {
            for i in 0..5000 {
                map2.insert(i, 1, &map2.guard());
            }
        });

        t1.join().unwrap();
        t2.join().unwrap();

        thread::sleep(Duration::from_micros(1000));
        let mut missed = 0;
        let guard = map.guard();
        for i in 0..5000 {
            let v = map.get(&i, &guard);
            if v.is_some() {
                assert!(v == Some(&0) || v == Some(&1));
            } else {
                missed += 1;
            }

            // let kv = map.get_key_value(&i, &guard).unwrap();
            // assert!(kv == (&i, &0) || kv == (&i, &1));
        }

        println!("missed {}", missed)
    }
}
