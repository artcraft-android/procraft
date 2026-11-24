import AVFAudio
import Foundation

/// The app's audio session, set up off the main thread. Its calls (`setCategory`, `setActive`)
/// go to the system's audio server and wait for its answer, which can hold the interface up for
/// as long as that takes: Xcode's "AVAudioSession Hang Risk". The timeline's sounds used to make
/// both on the main thread before every play and every scrubbed frame.
///
/// Here they are made once, on a queue of their own, and again only when another category is
/// wanted or the system took the session away. Everything but the queue's own work is the main
/// thread's.
enum AppAudio {
    private static let queue = DispatchQueue(label: "procraft-audio-session", qos: .userInitiated)
    /// The category the session is active under, as the main thread knows it; none before it is
    /// set up, or once the system has interrupted it.
    private(set) static var inForce: AVAudioSession.Category?
    private static var watching = false

    /// The session active under `category`, mixing with whatever else plays; then `work`, still
    /// off the main thread (starting an audio engine, say); then `then` on the main thread.
    /// With the session already so and no `work`, `then` runs at once.
    static func use(_ category: AVAudioSession.Category, work: (() -> Void)? = nil, then: @escaping () -> Void) {
        dispatchPrecondition(condition: .onQueue(.main))
        watch()
        let set = inForce != category
        if !set, work == nil {
            then()
            return
        }
        queue.async {
            if set {
                let session = AVAudioSession.sharedInstance()
                try? session.setCategory(category, options: [.mixWithOthers])
                try? session.setActive(true)
            }
            work?()
            DispatchQueue.main.async {
                if set {
                    inForce = category
                }
                then()
            }
        }
    }

    /// The system taking the session away (a call, another app's sound, its audio server
    /// starting again) means it is set up again when next wanted.
    private static func watch() {
        guard !watching else { return }
        watching = true
        let center = NotificationCenter.default
        center.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: .main) { note in
            let began = (note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt)
                .flatMap(AVAudioSession.InterruptionType.init(rawValue:)) == .began
            if began {
                inForce = nil
            }
        }
        center.addObserver(forName: AVAudioSession.mediaServicesWereResetNotification, object: nil, queue: .main) { _ in
            inForce = nil
        }
    }
}
