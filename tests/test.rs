#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    use rayon;
    use rayon::prelude::*;
    use syncmap::map::Map;


    const ITER: u64 = 32 * 1024;
    const ITERREMOVE: usize = 32 * 1024;


    #[test]
    #[cfg_attr(miri, ignore)]
    fn concurrent_insert() {
        let map = Arc::new(Map::<u64, u64>::new());


        let handles: Vec<_> = (0..10).map(|_| {
            let map1 = map.clone();
            thread::spawn(move || {
                let guard = map1.guard();
                for i in 0..ITER {
                    map1.insert(i, i, &guard);
                }
            })
        }).collect();


        for h in handles {
            h.join().unwrap()
        }


        thread::sleep(Duration::from_micros(1000));
        let mut missed = 0;
        let guard = map.guard();
        for i in 0..ITER {
            let v = map.get(&i, &guard);
            if v.is_some() {
                assert!(v == Some(&i));
            } else {
                missed += 1;
            }

            // let kv = map.get_key_value(&i, &guard).unwrap();
            // assert!(kv == (&i, &0) || kv == (&i, &1));
        }

        println!("missed {}", missed);

        println!("cpu {}", num_cpus::get())
    }

    #[test]
    fn concurrent_remove() {
        let map = Arc::new(Map::<u64, u64>::new());

        {
            let guard = map.guard();
            for i in 0..64 {
                map.insert(i, i, &guard);
            }
        }

        let map1 = map.clone();
        let t1 = std::thread::spawn(move || {
            let guard = map1.guard();
            for i in 0..64 {
                map1.remove(&i, &guard);
            }
        });
        let map2 = map.clone();
        let t2 = std::thread::spawn(move || {
            let guard = map2.guard();
            for i in 0..64 {
                map2.remove(&i, &guard);
            }
        });

        t1.join().unwrap();
        t2.join().unwrap();

        println!("size of {}",map.len());
        // after joining the threads, the map should be empty
        let guard = map.guard();
        for i in 0..64 {
            assert_eq!(map.get(&i, &guard),None);
        }
    }
}
