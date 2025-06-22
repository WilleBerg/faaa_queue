# Description
A very fast concurrent lock-free queue.

`faaa_queue` is a Rust implementation of the [FAAAQueue](https://concurrencyfreaks.blogspot.com/2016/11/faaarrayqueue-mpmc-lock-free-queue-part.html).

# Usage
```rust
use faaa_queue::FAAAQueue;

fn main() {
    let q: FAAAQueue<i32> = FAAAQueue::new();
    q.enqueue(1);
    assert_eq!(q.dequeue(), Some(1));
}
```
# Performance
The faaa_queue was benchmarked using the [rusty-benchmarking-framework](https://github.com/dcs-chalmers/rusty-benchmarking-framework).

It performs a lot better than all other concurrent queues benchmarked after 4 threads.

![Benchmark results](https://github.com/WilleBerg/faaa_queue/raw/d01c59aa9646256f3e1f7d4253b214707f0d31f6/images/six_subplot_comparison.png)

Here are results from three different benchmarks using varying tau (τ) values to control the enqueue/dequeue ratio. 

**Benchmark Setup:** Threads are synchronized to start simultaneously, with each thread alternating between enqueue and dequeue operations based on a random number r ∈ [0,1) - enqueueing when r > τ, dequeueing otherwise. Additional random operations simulate realistic workloads between each queue operation.

For implementation details, see the [rusty-benchmarking-framework](https://github.com/dcs-chalmers/rusty-benchmarking-framework).
