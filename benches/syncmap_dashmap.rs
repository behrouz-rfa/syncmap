/* Benchmarks from `dashmap` (https://github.com/xacrimon/dashmap),
 * adapted to flurry for comparison:
 *
 * This benchmark suite contains benchmarks for concurrent insertion
 * and retrieval (get).
 * Currently, this file provides two versions of each test, one which
 * follows the original implementation in using `par_iter().for_each()`,
 * which necessitates creating a new guard for each operation since
 * guards are not `Send + Sync`, and one version which uses threads
 * spawned in scopes. The latter version is able to create only one
 * guard per thread, but incurs overhead from the setup of the more
 * heavyweight threading environment.
 *
 * For the associated license information, please refer to dashmap.LICENSE.
 */

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use rayon;
use rayon::prelude::*;
use std::sync::Arc;
use syncmap::map::{Map};


/* DASHMAP */
const ITER: u64 = 32;

fn task_insert_syncmap_u64_u64_guard_every_it() -> Map<u64, u64> {
    let map = Map::new();

    (0..ITER).into_par_iter().for_each(|i| {
        let guard = map.guard();
        map.insert(i, i + 7, &guard);
    });
    map
}

fn insert_syncmap_u64_u64_guard_every_it(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert_syncmap_u64_u64_guard_every_it");
    group.throughput(Throughput::Elements(ITER as u64));
    let max = 2;

    for threads in 1..max {
        group.bench_with_input(
            BenchmarkId::from_parameter(threads),
            &threads,
            |b, &threads| {
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .unwrap();
                pool.install(|| b.iter(task_insert_syncmap_u64_u64_guard_every_it));
            },
        );
    }

    group.finish();
}

fn task_insert_syncmap_u64_u64_guard_once(threads: usize) -> Map<u64, u64> {
    let mut map = Arc::new(Map::new());
    let inc = ITER / (threads as u64);

    rayon::scope(|s| {
        for t in 1..=(threads as u64) {
            let m = Arc::clone(&map);
            s.spawn(move |_| {
                let start = t * inc;
                let guard = m.guard();
                for i in start..(start + inc) {
                    m.insert(i, i + 7, &guard);
                }
            });
        }
    });
    Arc::try_unwrap(map).unwrap()
}

fn insert_syncmap_u64_u64_guard_once(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert_syncmap_u64_u64_guard_once");
    group.throughput(Throughput::Elements(ITER as u64));
    let max = 2;

    for threads in 1..=max {
        group.bench_with_input(
            BenchmarkId::from_parameter(threads),
            &threads,
            |b, &threads| {
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .unwrap();
                pool.install(|| b.iter(|| task_insert_syncmap_u64_u64_guard_once(threads)));
            },
        );
    }

    group.finish();
}

fn task_get_syncmap_u64_u64_guard_every_it(map: &Map<u64, u64>) {
    (0..ITER).into_par_iter().for_each(|i| {
        let guard = map.guard();
        let item = map.get(&i, &guard);
        if item.is_some() {
            assert_eq!(item, Some(&(i + 7)));
        }
    });
}

fn get_syncmap_u64_u64_guard_every_it(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_syncmap_u64_u64_guard_every_it");
    group.throughput(Throughput::Elements(ITER as u64));
    let max = 3;
    for threads in 1..=max {
        let map = task_insert_syncmap_u64_u64_guard_every_it();

        group.bench_with_input(
            BenchmarkId::from_parameter(threads),
            &threads,
            |b, &threads| {
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .unwrap();
                pool.install(|| b.iter(|| task_get_syncmap_u64_u64_guard_every_it(&map)));
            },
        );
    }

    group.finish();
}



criterion_group!(
    benches,
    // insert_syncmap_u64_u64_guard_every_it,
    // insert_syncmap_u64_u64_guard_every_it,
get_syncmap_u64_u64_guard_every_it
    // get_syncmap_u64_u64_guard_every_it,

);
criterion_main!(benches);
