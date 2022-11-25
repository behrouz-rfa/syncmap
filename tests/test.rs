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
