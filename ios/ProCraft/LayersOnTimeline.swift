import Foundation

// On a timeline, the layers panel lists only the drawing being drawn on (the one at the playhead
// on the track drawn on): every other drawing's layers stay out of the way.

extension CanvasModel {
    /// Whether layer `l` is listed: off a timeline, every layer; on one, those of drawing `root`
    /// (itself, and everything inside it).
    func listed(_ l: LayerInfo, in root: UInt64?) -> Bool {
        guard let root else { return true }
        let byId = Dictionary(layers.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        var at: UInt64? = l.id
        for _ in 0 ... layers.count {
            guard let id = at else { return false }
            if id == root {
                return true
            }
            at = byId[id]?.parent
        }
        return false
    }
}
