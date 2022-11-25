# syncmap
<div>
  <div align="center" style="display: block; text-align: center;">
    <img src="https://camo.githubusercontent.com/734a3468bce992fbc3b729562d41c92f4912c99a/68747470733a2f2f7777772e727573742d6c616e672e6f72672f7374617469632f696d616765732f727573742d6c6f676f2d626c6b2e737667" height="120" width="120" />
  </div>
  <h1 align="center">syncmap</h1>
  <h4 align="center">syncmap is a fast, concurrent cache library  </h4>
</div>
syncmap is a fast, concurrent cache library built with a focus on performance and correctness.

The motivation to build syncmap comes from the sync.Map 
 in [Golang][].

[Golang]: https://pkg.go.dev/sync

## Summary

Map is like a  Hashmap but is safe for concurrent use by multiple thread without additional locking or coordination. Loads, stores, and deletes run in amortized constant time.

The Map type is specialized. Most code should use a plain Rust HashMap instead, with separate locking or coordination, for better type safety and to make it easier to maintain other invariants along with the map content.

The Map type is optimized for two common use cases: (1) when the entry for a given key is only ever written once but read many times, as in caches that only grow, or (2) when multiple thread read, write, and overwrite entries for disjoint sets of keys. In these two cases, use of a Map may significantly reduce lock contention compared to a Rust HashMap paired with a separate Mutex or RWMutex.

The zero Map is empty and ready for use. A Map must not be copied after first use.

In the terminology of the Go memory model, Map arranges that a write operation “synchronizes before” any read operation that observes the effect of the write, where read and write operations are defined as follows. Load, LoadAndDelete, LoadOrStore are read operations; Delete, LoadAndDelete, and Store are write operations; and LoadOrStore is a write operation when it returns loaded set to false.

## Status

syncmap is usable but still under active development. We expect it to be production ready in the near future.


## Usage

### Example

```rust
 use syncmap::map::Map;
fn main() {
  let cache = Map::new();

  let guard = cache.guard();
  cache.insert("key", "value1",  &guard);
 
  thread::sleep(Duration::from_millis(50));

  assert_eq!(cache.get(&"key", &guard), Some("value1"));

}
```

