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
    const ITER2: usize = 32 * 1024;


    #[test]
    #[cfg_attr(miri, ignore)]
    fn concurrent_insert() {
        let map = Arc::new(Map::<usize, usize>::new());


        let handles: Vec<_> = (0..20).map(|_| {
            let map1 = map.clone();
            thread::spawn(move || {
                let guard = map1.guard();
                for i in 0..ITER2 {
                    map1.insert(i, 0, &guard);
                }
            })
        }).collect();

        let handles2: Vec<_> = (0..20).map(|_| {
            let map1 = map.clone();
            thread::spawn(move || {
                let guard = map1.guard();
                for i in 0..ITER2 {
                    map1.insert(i, 0, &guard);
                }
            })
        }).collect();

        for h in handles {
            h.join().unwrap()
        }
        for h in handles2 {
            h.join().unwrap()
        }


        thread::sleep(Duration::from_micros(1000));
        let mut missed = 0;
        let guard = map.guard();
        for i in 0..ITER2 {
            let v = map.get(&i, &guard);
            if v.is_some() {
                assert!(v == Some(&0) || v == Some(&1));
            } else {
                missed += 1;
            }

            // let kv = map.get_key_value(&i, &guard).unwrap();
            // assert!(kv == (&i, &0) || kv == (&i, &1));
        }

        println!("missed {}", missed);
        println!("cpu {}", num_cpus::get())
    }
}
