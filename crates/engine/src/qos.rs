//! How urgent the system takes a thread's work to be, on Apple's systems (its quality of service
//! class). A thread made with `std::thread` has none of its own and runs as "default". When the
//! thread that made it then waits for it (work spread over a few threads, a session built on a
//! stack of its own), and that thread is one the user is waiting on, the system has the more
//! urgent thread waiting on a less urgent one: Xcode's "Hang Risk", and under load a real stall,
//! since nothing tells the scheduler to hurry the helper. So a helper takes up the class of the
//! thread that made it ([`Qos::current`], then [`Qos::adopt`] on the new thread), and the
//! threads the interface waits on all the time say what they are ([`Qos::INTERACTIVE`]).
//! Elsewhere all of this does nothing.
#![allow(unsafe_code)]

/// A thread's quality of service class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Qos(u32);

#[cfg(target_vendor = "apple")]
unsafe extern "C" {
    fn qos_class_self() -> u32;
    fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
}

impl Qos {
    /// Work the interface shows the result of at once (drawing the canvas).
    pub const INTERACTIVE: Qos = Qos(0x21);
    /// Work the user asked for and is waiting on.
    pub const INITIATED: Qos = Qos(0x19);
    // (A thread that mostly waits on the platform's own work, a video decoder's say, is best
    // left with no class: those threads run as "default", and a more urgent one waiting on them
    // is itself reported.)

    /// The calling thread's class.
    pub fn current() -> Qos {
        #[cfg(target_vendor = "apple")]
        {
            // SAFETY: takes nothing and only reads the calling thread's own class
            Qos(unsafe { qos_class_self() })
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            Qos(0)
        }
    }

    /// The calling thread takes this class up (nothing, where the class is unspecified: a thread
    /// that never had one).
    pub fn adopt(self) {
        #[cfg(target_vendor = "apple")]
        if self.0 != 0 {
            // SAFETY: sets only the calling thread's own class, to one the system handed out
            // (or one of the constants above); a refusal changes nothing
            let _ = unsafe { pthread_set_qos_class_self_np(self.0, 0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Qos;

    #[test]
    fn a_helper_thread_runs_as_urgently_as_the_thread_that_made_it() {
        // (a thread of the test harness's has no class; one that says what it is has)
        let made = std::thread::spawn(|| {
            Qos::INITIATED.adopt();
            let here = Qos::current();
            let helper = std::thread::spawn(move || {
                let before = Qos::current();
                here.adopt();
                (before, Qos::current())
            })
            .join()
            .unwrap_or((Qos(0), Qos(0)));
            (here, helper)
        })
        .join();
        let Ok((here, (before, after))) = made else { panic!("the thread panicked") };
        assert_eq!(after, here);
        if cfg!(target_vendor = "apple") {
            assert_eq!(here, Qos::INITIATED);
            assert_ne!(before, here, "a new thread has no class until it takes one");
        }
    }
}
