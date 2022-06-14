use std::marker::PhantomData;
use std::sync::{Arc};
use std::sync::atomic::{AtomicUsize, Ordering};
use parking_lot::{Condvar, Mutex};

pub struct RawSemaphore {
    capacity: usize,
    counter: AtomicUsize,
    lock: Mutex<()>,
    cond: Condvar
}

impl RawSemaphore {

    pub fn new(capacity: usize) -> RawSemaphore {
        RawSemaphore {
            capacity,
            counter: AtomicUsize::new(capacity),
            lock: Mutex::new(()),
            cond: Condvar::new()
        }
    }

    ///Attempt to acquire a spot in the semaphore
    /// Does not block
    pub fn try_acquire(&self) -> bool {

        //Attempt to acquire a spot in the semaphore
        //We use relaxed since all other variables in memory kind of don't matter to us until we have acquired?
        let mut currently_available = self.counter.load(Ordering::Relaxed);

        loop {

            if currently_available > 0 {
                //attempt to take the available slot
                match self.counter.compare_exchange_weak(currently_available, currently_available - 1,
                                                         Ordering::SeqCst, Ordering::SeqCst) {
                    Ok(_) => {
                        //Successfully acquired a slot in the semaphore
                        return true;
                    }
                    Err(new_curr) => {
                        //Failed, grab the new value returned without having to perform another load op
                        currently_available = new_curr;
                    }
                }
            } else {
                //There are no available spots
                return false;
            }
        }
    }

    ///Attempt to acquire a slot in the semaphore.
    ///Blocks until we are ready to receive
    pub fn acquire(&self) -> bool {
        //Attempt to acquire a spot in the semaphore
        //We use relaxed since all other variables in memory kind of don't matter to us until we have acquired?
        let mut currently_available = self.counter.load(Ordering::Relaxed);

        //If people miss use this semaphore, panic
        assert!(currently_available <= self.capacity);

        loop {
            if currently_available > 0 {
                match self.counter.compare_exchange_weak(currently_available, currently_available - 1,
                                                         Ordering::SeqCst, Ordering::SeqCst) {
                    Ok(_) => {
                        return true;
                    }
                    Err(new_curr) => {
                        currently_available = new_curr;
                    }
                }
            } else {
                let mut guard = self.lock.lock();

                currently_available = self.counter.load(Ordering::Relaxed);

                //See if a process has exited the semaphore in the mean time
                //Since we do this AFTER we acquire the lock and every process that exits when the currently_available == 0
                //Will try to lock that mutex so it can wake up the sleeping processes,
                //This should work 100% of the times
                if currently_available > 0 {
                    match self.counter.compare_exchange_weak(currently_available, currently_available - 1,
                                                             Ordering::SeqCst, Ordering::SeqCst) {
                        Ok(_) => {
                            return true;
                        }
                        Err(new_curr) => {
                            currently_available = new_curr;
                        }
                    }
                } else {
                    //There are no slots currently available, wait for processes to leave
                    self.cond.wait(&mut guard);
                    currently_available = self.counter.load(Ordering::SeqCst);
                }
            }
        }
    }

    ///Release a spot in the semaphore
    pub fn release(&self) {
        let previously_available = self.counter.fetch_add(1, Ordering::SeqCst);

        //If we are the firsts to exit after the semaphore was full, then we need
        //To wake up all sleeping processes so they can try to acquire it
        if previously_available == 0 {
            let _guard = self.lock.lock();

            self.cond.notify_all();
        }
    }

}

struct Semaphore<T> {
    sem: Arc<RawSemaphore>,
    phantom: PhantomData<T>
}

#[cfg(test)]
mod tests {
    use std::sync::Barrier;
    use std::time::Duration;
    use super::*;

    #[test]
    pub fn test_single_thread_try_acquire() {
        let capacity = 5;

        let sem = Arc::new(RawSemaphore::new(capacity));

        for _ in 0..capacity {
            assert!(sem.try_acquire())
        }

        assert!(!sem.try_acquire());

        sem.release();

        assert!(sem.try_acquire());

        for _ in 0..capacity {
            sem.release()
        }

        for _ in 0..capacity {
            assert!(sem.try_acquire())
        }
    }

    #[test]
    pub fn test_two_thread_try_acquire() {

        let capacity = 5;

        let sem = Arc::new(RawSemaphore::new(capacity));

        for _ in 0..capacity {
            assert!(sem.try_acquire())
        }

        assert!(!sem.try_acquire());

        let sem2 = sem.clone();

        let barrier = Arc::new(Barrier::new(2));
        let barrier_2 = Arc::clone(&barrier);

        let handle = std::thread::spawn(move || {

            assert!(!sem2.try_acquire());
            barrier.wait();

            println!("acquiring");
            println!("{}", sem2.acquire());
            println!("acquired");

            barrier.wait();

            std::thread::sleep(Duration::from_millis(1000));

            println!("releasing");
            sem2.release();
        });

        barrier_2.wait();

        println!("releasing");

        sem.release();

        //Wait for the other thread to acquire it
        barrier_2.wait();

        sem.acquire();

        println!("reacquired");

        handle.join().unwrap();
    }

}
