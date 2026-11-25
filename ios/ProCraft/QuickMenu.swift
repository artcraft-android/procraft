import SwiftUI

// glass: the buttons a dark tint over the canvas with a faint rim, the middle one a light tint
private let buttonFill = Color(argb: 0x9C28_2828)
private let buttonEdge = Color(argb: 0x40FF_FFFF)
private let buttonInk = Color(argb: 0xFFF7_F7F7)
private let titleFill = Color(argb: 0x94CF_CFCF)
private let titleInk = Color(argb: 0xFF0F_0F0F)
private let ringWidth: CGFloat = 171
private let ringHeight: CGFloat = 48
/// Where the buttons' centres are from the middle, clockwise from the top.
private let ringSlots: [CGPoint] = [
    CGPoint(x: 0, y: -69), CGPoint(x: 191, y: -39), CGPoint(x: 191, y: 39),
    CGPoint(x: 0, y: 69), CGPoint(x: -191, y: 39), CGPoint(x: -191, y: -39),
]
private let ringReachX: CGFloat = 191
private let ringReachY: CGFloat = 69

// the picker (as measured): a dark panel 220 wide, corners 30, an arrow 14 tall; capsule rows 47
// tall on a 50 pitch, 8.5 in from its sides, under a 55.5 tall title
private let pickerWidth: CGFloat = 220
private let pickerCorner: CGFloat = 30
private let pickerArrow: CGFloat = 14
private let pickerHeader: CGFloat = 55.5
private let pickerRowHeight: CGFloat = 47
private let pickerRowGap: CGFloat = 3
private let pickerRowPitch = pickerRowHeight + pickerRowGap
private let pickerInset: CGFloat = 8.5
private let pickerHeight = pickerHeader + pickerRowPitch * 6 - pickerRowGap + 6
private let pickerMargin: CGFloat = 8
/// How far below the list's top the chosen row shows.
private let pickerChosenAt: CGFloat = 89
private let pickerFill = Color(argb: 0xFF37_3737)
private let pickerRowFill = Color(argb: 0xFF2E_2E2E)
private let pickerRowInk = Color(argb: 0xFFC5_C5C5)
private let pickerTitleInk = Color(argb: 0xFFC4_C4C4)
/// The rows' labels (as measured by ink): their size, letter spacing, and how far below the row's
/// middle.
private let rowText: CGFloat = 15.1
private let rowTracking: CGFloat = -0.024 * 15.1
private let rowTextDrop: CGFloat = 0.75

// the menus' popover (as measured): the picker's panel, 249 tall, 17 off the middle with its
// arrow's tip 5.5 off it; rows from 54 under its top, 8 in; a plus 21 across and 3.6 thick, 30 from
// its right and 28 from its top; a row swiped 70 shows a red Delete 63 x 42, 4.5 in from its end
private let menusHeight: CGFloat = 249
private let menusOff: CGFloat = 17
private let menusTip: CGFloat = 5.5
private let menusRowsAt: CGFloat = 54
private let menusInset: CGFloat = 8
private let plusLength: CGFloat = 21
private let plusWeight: CGFloat = 3.6
private let plusInk = Color(argb: 0xFFC0_C0C0)
private let swipeOpen: CGFloat = 70
private let deleteWidth: CGFloat = 63
private let deleteHeight: CGFloat = 42
private let deleteIn: CGFloat = 4.5
private let deleteFill = Color(argb: 0xFFFE_4346)

// a touch on the ring (as measured): the button the finger is at is blue and a fifth larger, and
// follows the finger past it with resistance, never further than its reach
private let pullGive: CGFloat = 0.55
private let pullReach: CGFloat = 220
/// A touch landing no further than this from the ring is on it; one that has gone this far is no
/// tap; a button held still this long lists what it can be set to.
private let ringGrab: CGFloat = 30
private let ringTapSlop: CGFloat = 10
private let ringHold: TimeInterval = 0.4
/// A finger setting off from the middle has chosen its way once it is this far from where it
/// landed. The finger that opened the menu by sliding off is further than `ringSlidOpen` from
/// there when the menu first sees it (the core's tap slop is 24); one held still is nearer.
private let ringFlick: CGFloat = 12
private let ringSlidOpen: CGFloat = 22

/// How far `p` is from a button (or the middle) whose centre is `c`: nothing on it.
private func ringGap(_ p: CGPoint, _ c: CGPoint) -> CGFloat {
    hypot(max(abs(p.x - c.x) - ringWidth / 2, 0), max(abs(p.y - c.y) - ringHeight / 2, 0))
}

/// The button a finger at `p` is at, of the first `count` round `centre`: the nearest, however far
/// past it the finger has gone; none in the middle.
private func ringButton(at p: CGPoint, centre: CGPoint, count: Int) -> Int? {
    var nearest: Int?
    var gap = ringGap(p, centre)
    for i in 0 ..< min(count, ringSlots.count) {
        let g = ringGap(p, CGPoint(x: centre.x + ringSlots[i].x, y: centre.y + ringSlots[i].y))
        if g < gap {
            gap = g
            nearest = i
        }
    }
    return nearest
}

/// The button a finger gone `v` from the middle is making for: the one it is on, else the one most
/// nearly that way.
private func ringButton(towards v: CGSize, count: Int) -> Int? {
    let n = min(count, ringSlots.count)
    if let on = (0 ..< n).first(where: { ringGap(CGPoint(x: v.width, y: v.height), ringSlots[$0]) == 0 }) {
        return on
    }
    let far = hypot(v.width, v.height)
    guard far > 0 else { return nil }
    var nearest: Int?
    var most = -CGFloat.infinity
    for i in 0 ..< n {
        let slot = ringSlots[i]
        let along = (v.width * slot.x + v.height * slot.y) / (far * hypot(slot.x, slot.y))
        if along > most {
            most = along
            nearest = i
        }
    }
    return nearest
}

/// A pull of (`dx`, `dy`) as a button follows it: less and less the further the finger goes.
private func ringResist(_ dx: CGFloat, _ dy: CGFloat) -> CGSize {
    let far = hypot(dx, dy)
    guard far > 0 else { return .zero }
    let k = pullReach * (1 - 1 / (far * pullGive / pullReach + 1)) / far
    return CGSize(width: dx * k, height: dy * k)
}

/// How far a finger at `p` pulls the button whose centre is `c`: towards the finger from the line
/// along the button's middle.
private func ringPull(_ p: CGPoint, _ c: CGPoint) -> CGSize {
    let half = (ringWidth - ringHeight) / 2
    return ringResist(p.x - min(max(p.x, c.x - half), c.x + half), p.y - c.y)
}

/// A touch on the open QuickMenu.
private struct RingTouch {
    /// Where it landed, and where the menu first saw it (the finger that opened the menu had
    /// landed on the canvas before).
    let from: CGPoint
    let seen: CGPoint
    /// It landed off the ring (or with a list open): lifted, it closes what is open.
    let outside: Bool
    /// It is the finger that opened the menu, still down on the canvas.
    let canvas: Bool
    /// It landed in the middle (or opened the menu, which is round where it landed): it lights
    /// the button it sets off towards, which goes the way it is pulled.
    let flick: Bool
    /// It has gone past a tap's slop.
    var moved = false
    /// Held on a button, it opened the button's picker: it does no more.
    var spent = false
}

/// The QuickMenu: its name in the middle of where it was opened, its six actions round it. A
/// finger slid to an action lights it, and let go there runs it and closes the menu (so does a
/// tap on one); let go in the middle it does nothing. The finger that opened it by sliding off
/// where it landed goes straight on: the action it set off towards is lit as the menu shows. A
/// tap anywhere else closes it, a tap on the middle lists the menus, and a button held lists the
/// actions it can be set to.
struct QuickMenuView: View {
    let model: CanvasModel
    let screen: CGSize

    /// Where it shows, kept while it draws back in; how far it has grown.
    @State private var at: CGPoint?
    @State private var shown: CGFloat = 0
    /// The button whose action is being chosen.
    @State private var choosing: Int?
    /// The menus' popover (the middle tapped): open, the menus added while it shows, the row blue.
    @State private var menusOpen = false
    @State private var fresh: [Int] = []
    @State private var marked: Int?
    /// The touch on it, the button the finger is at, and how far the finger has pulled it.
    @State private var touch: RingTouch?
    @State private var lit: Int?
    @State private var pull: CGSize = .zero
    /// The wait for a button held still.
    @State private var holding: DispatchWorkItem?

    private func closeMenus() {
        menusOpen = false
        fresh = []
    }

    /// A tap elsewhere closes the picker or the menus if open, else the menu.
    private func tapOutside() {
        if choosing != nil {
            choosing = nil
        } else if menusOpen {
            closeMenus()
        } else {
            model.quickMenuAt = nil
        }
    }

    /// The ring's middle, opened at `point`: kept on the screen.
    private func ringCentre(_ point: CGPoint) -> CGPoint {
        CGPoint(
            x: min(max(point.x, ringReachX + ringWidth / 2), screen.width - ringReachX - ringWidth / 2),
            y: min(max(point.y, ringReachY + ringHeight), screen.height - ringReachY - ringHeight)
        )
    }

    private var buttons: Int {
        min(QuickMenus.shared.menu?.slots.count ?? 0, ringSlots.count)
    }

    /// No touch on it any more, and no button lit (at once, not drawn back).
    private func dropTouch() {
        holding?.cancel()
        holding = nil
        touch = nil
        var still = Transaction()
        still.disablesAnimations = true
        withTransaction(still) {
            lit = nil
            pull = .zero
        }
    }

    /// A touch lands at `p`.
    private func touchLanded(_ p: CGPoint, centre: CGPoint) {
        holding?.cancel()
        holding = nil
        guard choosing == nil, !menusOpen else {
            touch = RingTouch(from: p, seen: p, outside: true, canvas: false, flick: false)
            return
        }
        let gaps = ([.zero] + ringSlots.prefix(buttons)).map {
            ringGap(p, CGPoint(x: centre.x + $0.x, y: centre.y + $0.y))
        }
        let outside = (gaps.min() ?? 0) > ringGrab
        touch = RingTouch(from: p, seen: p, outside: outside, canvas: false, flick: ringGap(p, centre) == 0)
        guard !outside, let i = ringButton(at: p, centre: centre, count: buttons) else { return }
        // kept still on a button, it lists what the button can be set to
        let wait = DispatchWorkItem {
            guard let held = touch, held.from == p, !held.moved, !held.spent, lit == i else { return }
            touch?.spent = true
            lit = nil
            pull = .zero
            choosing = i
        }
        holding = wait
        DispatchQueue.main.asyncAfter(deadline: .now() + ringHold, execute: wait)
    }

    /// The touch is at `p`: the button there lights and goes after the finger.
    private func touchSlid(_ p: CGPoint, centre: CGPoint) {
        guard var slid = touch, !slid.outside, !slid.spent else { return }
        if !slid.moved, hypot(p.x - slid.seen.x, p.y - slid.seen.y) > ringTapSlop {
            slid.moved = true
            touch = slid
            holding?.cancel()
            holding = nil
        }
        // the finger that opened the menu held still lights nothing until it sets off
        guard slid.moved || !slid.canvas else { return }
        if slid.flick {
            // from the middle: the button it makes for, pulled the way it goes
            let gone = CGSize(width: p.x - slid.from.x, height: p.y - slid.from.y)
            let i = hypot(gone.width, gone.height) < ringFlick ? nil : ringButton(towards: gone, count: buttons)
            lit = i
            pull = i == nil ? .zero : ringResist(gone.width, gone.height)
        } else {
            let i = ringButton(at: p, centre: centre, count: buttons)
            lit = i
            pull = i.map { ringPull(p, CGPoint(x: centre.x + ringSlots[$0].x, y: centre.y + ringSlots[$0].y)) } ?? .zero
        }
    }

    /// The touch lifts: on a button, the button runs and the menu goes (the button still lit).
    private func touchLifted(opened point: CGPoint, centre: CGPoint) {
        holding?.cancel()
        holding = nil
        guard let lifted = touch else { return }
        touch = nil
        if lifted.outside {
            tapOutside()
        } else if lifted.spent {
            return
        } else if let i = lit {
            model.quickMenuAt = nil
            model.runQuickSlot(i, at: point)
        } else if !lifted.moved, !lifted.canvas, ringGap(lifted.from, centre) == 0 {
            // the middle tapped lists the menus
            menusOpen = true
            marked = QuickMenus.shared.active
        }
    }

    /// The finger that opened the menu, still down on the canvas, slides to a button and lets go.
    private func canvasFinger(_ step: FingerStep?) {
        guard let at = model.quickMenuAt else { return }
        let centre = ringCentre(at)
        switch step {
        case let .slid(p):
            if touch == nil, choosing == nil, !menusOpen {
                // sliding off it opened the menu, and is on its way already
                touch = RingTouch(
                    from: at, seen: p, outside: false, canvas: true, flick: true,
                    moved: hypot(p.x - at.x, p.y - at.y) > ringSlidOpen
                )
            }
            if touch?.canvas == true {
                touchSlid(p, centre: centre)
            }
        case .lifted:
            if touch?.canvas == true {
                touchLifted(opened: at, centre: centre)
            }
        case .takenBack, nil:
            if touch?.canvas == true {
                dropTouch()
            }
        }
    }

    private func touches(_ point: CGPoint, centre: CGPoint) -> some Gesture {
        DragGesture(minimumDistance: 0, coordinateSpace: .local)
            .onChanged { drag in
                // another finger with the one that opened it still down: they meant the canvas
                if model.quickMenuHeld {
                    model.loneFinger(.takenBack)
                }
                guard model.quickMenuAt != nil else { return }
                // (a touch the system took back never lifted: the next one starts afresh)
                if touch == nil || touch?.canvas == true || touch?.from != drag.startLocation {
                    touchLanded(drag.startLocation, centre: centre)
                }
                touchSlid(drag.location, centre: centre)
            }
            .onEnded { drag in
                guard touch?.from == drag.startLocation else { return }
                touchSlid(drag.location, centre: centre)
                touchLifted(opened: point, centre: centre)
            }
    }

    var body: some View {
        ZStack(alignment: .topLeading) {
            if let at {
                let centre = ringCentre(at)
                Color.clear
                    .contentShape(Rectangle())
                    .gesture(touches(at, centre: centre))
                ring(at, centre: centre)
                    .onAppear {
                        #if DEBUG
                            showForCapture()
                        #endif
                        withAnimation(Motion.quickMenuOpen) {
                            shown = 1
                        }
                    }
            }
        }
        .frame(width: screen.width, height: screen.height, alignment: .topLeading)
        .allowsHitTesting(model.quickMenuAt != nil)
        .onChange(of: model.quickMenuAt, initial: true) { _, target in
            if let target {
                dropTouch()
                if at != nil {
                    // back open while it drew in
                    withAnimation(Motion.quickMenuOpen) {
                        shown = 1
                    }
                }
                at = target
                // the finger that slid off to open it is on its way to a button already
                canvasFinger(model.quickMenuFinger)
            } else if at != nil {
                holding?.cancel()
                holding = nil
                touch = nil
                choosing = nil
                closeMenus()
                withAnimation(Motion.quickMenuClose, completionCriteria: .logicallyComplete) {
                    shown = 0
                } completion: {
                    if model.quickMenuAt == nil {
                        at = nil
                        lit = nil
                        pull = .zero
                    }
                }
            }
        }
        .onChange(of: model.quickMenuFinger) { _, step in
            canvasFinger(step)
        }
    }

    #if DEBUG
        private func showForCapture() {
            switch ScreenCapture.quickShows {
            case "menus":
                menusOpen = true
                marked = QuickMenus.shared.active
            case "picker":
                choosing = 1
            default:
                break
            }
        }
    #endif

    private func ring(_ point: CGPoint, centre: CGPoint) -> some View {
        let menus = QuickMenus.shared
        let slots = menus.menu?.slots ?? []
        let grown = Motion.quickMenuStartScale + (1 - Motion.quickMenuStartScale) * shown
        return ZStack(alignment: .topLeading) {
            // the ring grows out of its middle as it fades in; the touches on it are the menu's
            ZStack(alignment: .topLeading) {
                Text(verbatim: menus.menu?.name ?? "")
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(titleInk)
                    .lineLimit(1)
                    .frame(width: ringWidth, height: ringHeight)
                    .background(Capsule().fill(titleFill))
                    .offset(x: centre.x - ringWidth / 2, y: centre.y - ringHeight / 2)
                ForEach(Array(slots.indices), id: \.self) { i in
                    if ringSlots.indices.contains(i) {
                        button(i, centre: centre)
                    }
                }
            }
            .frame(width: screen.width, height: screen.height, alignment: .topLeading)
            .scaleEffect(
                grown,
                anchor: UnitPoint(x: centre.x / max(screen.width, 1), y: centre.y / max(screen.height, 1))
            )
            .opacity(shown)
            .allowsHitTesting(false)
            if let i = choosing, ringSlots.indices.contains(i) {
                let top = centre.y + ringSlots[i].y - ringHeight / 2
                ActionPicker(
                    current: slots.indices.contains(i) ? slots[i] : .none,
                    brush: model.quickSlotBrush(i),
                    sets: model.brushSets,
                    centreX: centre.x + ringSlots[i].x,
                    buttonTop: top,
                    buttonBottom: top + ringHeight,
                    screen: screen,
                    onBrush: {
                        model.setQuickSlotBrush(i, $0)
                        choosing = nil
                    },
                    onPick: {
                        menus.setSlot(i, $0)
                        choosing = nil
                    }
                )
            }
            if menusOpen {
                let left = centre.x - ringWidth / 2
                let top = centre.y - ringHeight / 2
                MenusPopover(
                    menu: CGRect(x: left, y: top, width: ringWidth, height: ringHeight),
                    screen: screen,
                    fresh: fresh,
                    marked: marked,
                    onAdd: {
                        // it shows, but the active one stays until a row is chosen
                        fresh.insert(menus.add(), at: 0)
                        marked = nil
                    },
                    onChoose: {
                        menus.choose($0)
                        marked = $0
                    },
                    onRename: { menus.rename($0, $1) },
                    onDelete: { i in
                        menus.delete(i)
                        fresh = fresh.filter { $0 != i }.map { $0 > i ? $0 - 1 : $0 }
                        marked = nil
                    }
                )
            }
        }
    }

    /// Button `i` in its place: blue and larger with the finger at it, and pulled after the finger.
    private func button(_ i: Int, centre: CGPoint) -> some View {
        let on = lit == i
        let pulled = on ? pull : .zero
        return Text(verbatim: model.quickSlotLabel(i))
            .font(.system(size: 16, weight: .semibold))
            .foregroundStyle(on ? Color.white : buttonInk)
            .lineLimit(1)
            .multilineTextAlignment(.center)
            .frame(width: ringWidth, height: ringHeight)
            .background(Capsule().fill(on ? Pc.accent : buttonFill))
            .overlay(Capsule().strokeBorder(on ? Color.clear : buttonEdge, lineWidth: 1))
            .scaleEffect(on ? Motion.quickMenuLitScale : 1)
            .offset(pulled)
            .animation(Motion.quickMenuFollow, value: on)
            .animation(Motion.quickMenuFollow, value: pulled)
            .offset(
                x: centre.x + ringSlots[i].x - ringWidth / 2,
                y: centre.y + ringSlots[i].y - ringHeight / 2
            )
            .zIndex(on ? 1 : 0)
    }
}

/// A row's label in the picker and the menus' popover.
private func rowLabel(_ text: String, on: Bool) -> some View {
    Text(verbatim: text)
        .font(.system(size: rowText, weight: on ? .semibold : .regular))
        .tracking(rowTracking)
        .foregroundStyle(on ? Color.white : pickerRowInk)
        .lineLimit(1)
        .offset(y: rowTextDrop)
}

/// A page of a held button's picker.
private enum PickerPage {
    case actions, adjustments, brushSets, brushes
}

/// A row of the picker: what it says, whether it opens a list of its own, and what a tap does.
private struct PickerRow {
    let label: String
    let more: Bool
    let tap: () -> Void
}

/// A held QuickMenu button's picker, hung from the button by an arrow (above it when there is no
/// room below): every action by name, the button's own blue, scrolled to it. Adjustments opens
/// their own list; Select brush the brush sets, then a set's brushes. A row tapped sets the button.
private struct ActionPicker: View {
    let current: QuickAction
    let brush: BrushInfo?
    let sets: [BrushSetInfo]
    let centreX: CGFloat
    let buttonTop: CGFloat
    let buttonBottom: CGFloat
    let screen: CGSize
    let onBrush: (BrushInfo) -> Void
    let onPick: (QuickAction) -> Void

    @State private var page: PickerPage
    @State private var set = 0

    init(
        current: QuickAction, brush: BrushInfo?, sets: [BrushSetInfo], centreX: CGFloat, buttonTop: CGFloat,
        buttonBottom: CGFloat, screen: CGSize, onBrush: @escaping (BrushInfo) -> Void,
        onPick: @escaping (QuickAction) -> Void
    ) {
        self.current = current
        self.brush = brush
        self.sets = sets
        self.centreX = centreX
        self.buttonTop = buttonTop
        self.buttonBottom = buttonBottom
        self.screen = screen
        self.onBrush = onBrush
        self.onPick = onPick
        _page = State(initialValue: quickActionAdjustment(action: current) != nil ? .adjustments : .actions)
    }

    /// The page's title and where its way back goes.
    private var heading: (title: String, back: PickerPage?) {
        switch page {
        case .actions: ("Set Action", nil)
        case .adjustments: ("Adjustments", .actions)
        case .brushSets: ("Brush Set", .actions)
        case .brushes: ("Brushes", .brushSets)
        }
    }

    /// The actions and adjustments listed, less a built-in plugin's that is off.
    private var listed: [QuickAction?] {
        quickActionsListed().filter { $0.map(PluginsModel.shared.shows) ?? true }
    }

    private var adjustmentsListed: [QuickAction] {
        quickAdjustmentsListed().filter(PluginsModel.shared.shows)
    }

    private var rows: [PickerRow] {
        switch page {
        case .actions:
            listed.map { a in
                guard let a else {
                    return PickerRow(label: "Adjustments", more: true) { page = .adjustments }
                }
                let label = quickActionLabel(action: a)
                if a == .brushes {
                    return PickerRow(label: label, more: true) {
                        set = Int(brush?.set ?? 0)
                        page = .brushSets
                    }
                }
                return PickerRow(label: label, more: false) { onPick(a) }
            }
        case .adjustments:
            adjustmentsListed.map { a in
                PickerRow(label: quickActionLabel(action: a), more: false) { onPick(a) }
            }
        case .brushSets:
            sets.enumerated().map { s, info in
                PickerRow(label: info.name, more: true) {
                    set = s
                    page = .brushes
                }
            }
        case .brushes:
            (sets.indices.contains(set) ? sets[set].brushes : []).map { b in
                PickerRow(label: b.name, more: false) { onBrush(b) }
            }
        }
    }

    /// The one row blue: in a list of its own, the button's, else the first.
    private var chosen: Int {
        switch page {
        case .actions:
            listed.firstIndex(of: current) ?? -1
        case .adjustments:
            adjustmentsListed.firstIndex(of: current) ?? 0
        case .brushSets:
            Int(brush?.set ?? 0)
        case .brushes:
            brush.flatMap { Int($0.set) == set ? Int($0.index) : nil } ?? 0
        }
    }

    /// The chosen row a little below the list's top; the first two stay where they are.
    private func show(_ reader: ScrollViewProxy) {
        let list = pickerHeight - pickerHeader
        let row = chosen
        DispatchQueue.main.async {
            if row >= 2 {
                reader.scrollTo(row, anchor: UnitPoint(x: 0.5, y: pickerChosenAt / (list - pickerRowHeight)))
            } else {
                reader.scrollTo(0, anchor: .top)
            }
        }
    }

    var body: some View {
        let below = buttonBottom + pickerArrow + pickerHeight <= screen.height - pickerMargin
        let left = min(max(centreX - pickerWidth / 2, pickerMargin), max(screen.width - pickerWidth - pickerMargin, pickerMargin))
        let top = below ? buttonBottom + pickerArrow : buttonTop - pickerArrow - pickerHeight
        let shape = RoundedRectangle(cornerRadius: pickerCorner, style: .circular)
        let head = heading
        let listed = rows
        let blue = chosen
        ZStack(alignment: .topLeading) {
            // the arrow, its point on the button
            Canvas { context, _ in
                let tip = below ? buttonBottom : buttonTop
                let base = below ? tip + pickerArrow + 1 : tip - pickerArrow - 1
                var path = Path()
                path.move(to: CGPoint(x: centreX, y: tip))
                path.addLine(to: CGPoint(x: centreX + pickerArrow, y: base))
                path.addLine(to: CGPoint(x: centreX - pickerArrow, y: base))
                path.closeSubpath()
                context.fill(path, with: .color(pickerFill))
            }
            .frame(width: screen.width, height: screen.height)
            .allowsHitTesting(false)
            VStack(spacing: 0) {
                // the title; in a list of its own a way back
                HStack(alignment: .top, spacing: 4) {
                    if head.back != nil {
                        PcIconView(PcIcons.chevronLeft, size: 24, color: pickerTitleInk)
                    }
                    Text(verbatim: head.title)
                        .font(.system(size: 19.3, weight: .bold))
                        .foregroundStyle(pickerTitleInk)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                }
                .padding(.leading, head.back != nil ? 13 : 21)
                .padding(.top, 15)
                .frame(height: pickerHeader, alignment: .top)
                .contentShape(Rectangle())
                .onTapGesture {
                    if let back = head.back {
                        page = back
                    }
                }
                ScrollViewReader { reader in
                    ScrollView(.vertical, showsIndicators: false) {
                        VStack(spacing: pickerRowGap) {
                            ForEach(listed.indices, id: \.self) { k in
                                row(listed[k], on: k == blue)
                                    .id(k)
                            }
                        }
                        .padding(.horizontal, pickerInset)
                        .padding(.bottom, 6)
                    }
                    .onAppear { show(reader) }
                    .onChange(of: page) { show(reader) }
                }
            }
            .frame(width: pickerWidth, height: pickerHeight)
            .background(shape.fill(pickerFill))
            .clipShape(shape)
            .contentShape(shape)
            .onTapGesture {}
            .shadow(color: .black.opacity(0.35), radius: 12)
            .offset(x: left, y: top)
        }
    }

    private func row(_ row: PickerRow, on: Bool) -> some View {
        ZStack {
            rowLabel(row.label, on: on)
                .frame(maxWidth: .infinity, alignment: .leading)
            if row.more {
                PcIconView(PcIcons.chevronRight, size: 18, color: on ? Color.white : pickerRowInk)
                    .frame(maxWidth: .infinity, alignment: .trailing)
            }
        }
        .padding(.horizontal, 16)
        .frame(height: pickerRowHeight)
        .background(Capsule().fill(on ? Pc.accent : pickerRowFill))
        .contentShape(Capsule())
        .onTapGesture(perform: row.tap)
    }
}

/// The QuickMenu's menus, hung from the middle (below it where it fits, else above, else beside): a
/// plus adds one and shows it, a row tapped makes it the active one (tapped again, it is renamed
/// in place), a row swiped left shows Delete (swiped most of the way, it goes at once). Menus added
/// while it shows are listed first.
private struct MenusPopover: View {
    /// Where the middle is.
    let menu: CGRect
    let screen: CGSize
    let fresh: [Int]
    let marked: Int?
    let onAdd: () -> Void
    let onChoose: (Int) -> Void
    let onRename: (Int, String) -> Void
    let onDelete: (Int) -> Void

    /// The row swiped and how far.
    @State private var swiped: Int?
    @State private var offset: CGFloat = 0
    /// The menu being renamed: its row, the active one, tapped again.
    @State private var renaming: Int?

    /// Where the panel goes, and which of below, above, right, left that is.
    private var place: (at: CGPoint, side: Int) {
        let places = [
            CGPoint(x: menu.midX - pickerWidth / 2, y: menu.maxY + menusOff),
            CGPoint(x: menu.midX - pickerWidth / 2, y: menu.minY - menusOff - menusHeight),
            CGPoint(x: menu.maxX + menusOff, y: menu.midY - menusHeight / 2),
            CGPoint(x: menu.minX - menusOff - pickerWidth, y: menu.midY - menusHeight / 2),
        ]
        let side = places.firstIndex {
            $0.x >= pickerMargin && $0.y >= pickerMargin && $0.x + pickerWidth <= screen.width - pickerMargin
                && $0.y + menusHeight <= screen.height - pickerMargin
        } ?? 0
        return (places[side], side)
    }

    /// The arrow, its point just off the middle.
    private func arrow(_ at: CGPoint, _ side: Int) -> Path {
        let a = pickerArrow
        var path = Path()
        let points: [CGPoint] = switch side {
        case 0:
            [
                CGPoint(x: menu.midX, y: menu.maxY + menusTip),
                CGPoint(x: menu.midX + a, y: at.y + 1), CGPoint(x: menu.midX - a, y: at.y + 1),
            ]
        case 1:
            [
                CGPoint(x: menu.midX, y: menu.minY - menusTip),
                CGPoint(x: menu.midX - a, y: at.y + menusHeight - 1),
                CGPoint(x: menu.midX + a, y: at.y + menusHeight - 1),
            ]
        case 2:
            [
                CGPoint(x: menu.maxX + menusTip, y: menu.midY),
                CGPoint(x: at.x + 1, y: menu.midY - a), CGPoint(x: at.x + 1, y: menu.midY + a),
            ]
        default:
            [
                CGPoint(x: menu.minX - menusTip, y: menu.midY),
                CGPoint(x: at.x + pickerWidth - 1, y: menu.midY + a),
                CGPoint(x: at.x + pickerWidth - 1, y: menu.midY - a),
            ]
        }
        path.addLines(points)
        path.closeSubpath()
        return path
    }

    var body: some View {
        let placed = place
        let menus = QuickMenus.shared.sets
        let shape = RoundedRectangle(cornerRadius: pickerCorner, style: .circular)
        // the menus added this time first, newest first, then the rest as kept
        let order = fresh.filter { menus.indices.contains($0) } + menus.indices.filter { !fresh.contains($0) }
        ZStack(alignment: .topLeading) {
            Canvas { context, _ in
                context.fill(arrow(placed.at, placed.side), with: .color(pickerFill))
            }
            .frame(width: screen.width, height: screen.height)
            .allowsHitTesting(false)
            ZStack(alignment: .topLeading) {
                Text(verbatim: "QuickMenu")
                    .font(.system(size: 19.3, weight: .bold))
                    .foregroundStyle(pickerTitleInk)
                    .lineLimit(1)
                    .padding(.leading, 21)
                    .padding(.top, 16)
                // the plus, its bars' ends round
                ZStack {
                    Capsule().fill(plusInk).frame(width: plusLength, height: plusWeight)
                    Capsule().fill(plusInk).frame(width: plusWeight, height: plusLength)
                }
                .frame(width: 44, height: 44)
                .contentShape(Rectangle())
                .onTapGesture(perform: onAdd)
                .offset(x: pickerWidth - 30 - 22, y: 28 - 22)
                ScrollView(.vertical, showsIndicators: false) {
                    VStack(spacing: pickerRowGap) {
                        ForEach(order, id: \.self) { i in
                            row(i, name: menus[i].name, swipes: menus.count > 1)
                        }
                    }
                    .padding(.horizontal, menusInset)
                    .padding(.bottom, 6)
                }
                .padding(.top, menusRowsAt)
                .padding(.bottom, 9.5)
            }
            .frame(width: pickerWidth, height: menusHeight, alignment: .topLeading)
            .background(shape.fill(pickerFill))
            .clipShape(shape)
            .contentShape(shape)
            .onTapGesture {}
            .shadow(color: .black.opacity(0.35), radius: 12)
            .offset(x: placed.at.x, y: placed.at.y)
        }
    }

    private func row(_ i: Int, name: String, swipes: Bool) -> some View {
        let on = marked == i
        let moved = swiped == i ? offset : 0
        let width = pickerWidth - menusInset * 2
        return ZStack(alignment: .trailing) {
            // the Delete, shown as far as the row has moved off it
            Text(verbatim: "Delete")
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(Color.white)
                .lineLimit(1)
                .frame(width: deleteWidth, height: deleteHeight)
                .background(Capsule().fill(deleteFill))
                .mask(alignment: .trailing) {
                    Rectangle().frame(width: min(max(-moved - deleteIn - 2.5, 0), deleteWidth))
                }
                .contentShape(Rectangle())
                .onTapGesture {
                    if moved <= -swipeOpen {
                        swiped = nil
                        offset = 0
                        onDelete(i)
                    }
                }
                .padding(.trailing, deleteIn)
            Group {
                if renaming == i {
                    InlineName(name: name, color: .white, fontSize: rowText, weight: .semibold) {
                        onRename(i, $0)
                        renaming = nil
                    }
                    .offset(y: rowTextDrop)
                } else {
                    rowLabel(name, on: on)
                }
            }
            .padding(.horizontal, 16)
            .frame(width: width, height: pickerRowHeight, alignment: .leading)
            .background(Capsule().fill(on ? Pc.accent : pickerRowFill))
            .contentShape(Capsule())
            .offset(x: moved)
            // a tap closes a row left open, renames the active menu, else chooses the menu
            .onTapGesture {
                if swiped != nil {
                    withAnimation(Motion.swipeSettle) {
                        swiped = nil
                        offset = 0
                    }
                } else if marked == i {
                    renaming = i
                } else {
                    onChoose(i)
                }
            }
            .simultaneousGesture(swipe(i, width: width, enabled: swipes))
        }
        .frame(width: width, height: pickerRowHeight)
    }

    private func swipe(_ i: Int, width: CGFloat, enabled: Bool) -> some Gesture {
        DragGesture(minimumDistance: 14)
            .onChanged { drag in
                guard enabled, abs(drag.translation.width) > abs(drag.translation.height) || swiped == i else { return }
                let from = swiped == i && offset <= -swipeOpen ? -swipeOpen : 0
                swiped = i
                offset = min(max(from + drag.translation.width, -width), 0)
            }
            .onEnded { _ in
                guard swiped == i else { return }
                // past most of the row it goes; past half the Delete it stays open
                if offset < -width * 0.6 {
                    swiped = nil
                    offset = 0
                    onDelete(i)
                } else {
                    withAnimation(Motion.swipeSettle) {
                        if offset < -swipeOpen / 2 {
                            offset = -swipeOpen
                        } else {
                            swiped = nil
                            offset = 0
                        }
                    }
                }
            }
    }
}

private func panelFill() -> Color {
    Color(argb: Look.shared.light ? 0xFFE2_E2E2 : 0xFF48_4848)
}

private func panelTitleInk() -> Color {
    Color(argb: Look.shared.light ? 0xFF39_3939 : 0xFFC5_C5C5)
}

private func panelButtonInk() -> Color {
    Color(argb: Look.shared.light ? 0xFF4A_4A4A : 0xFFA8_A8A8)
}

private func panelCloseFill() -> Color {
    Color(argb: Look.shared.light ? 0xFFCF_CFCF : 0xFF2A_2A2A)
}

/// The Copy & Paste panel (three fingers swiped down): Cut, Copy, Copy All, Duplicate, Cut & Paste
/// and Paste, a little below the middle of the screen; a tap off it or its cross closes it.
struct CopyPastePanel: View {
    let model: CanvasModel
    let screen: CGSize

    private func act(_ f: @escaping () -> Void) -> () -> Void {
        {
            model.copyPasteOpen = false
            f()
        }
    }

    private func button(_ icon: PcIcon, _ label: String, enabled: Bool = true, action: @escaping () -> Void) -> some View {
        let ink = panelButtonInk().opacity(enabled ? 1 : 0.4)
        // the icon (a 50 frame) 28 across, centred 17 down; the name from 34
        return ZStack(alignment: .top) {
            PcIconView(icon, size: 28, color: ink)
                .padding(.top, 3)
            Text(verbatim: label)
                .font(.system(size: 13))
                .foregroundStyle(ink)
                .lineLimit(1)
                .fixedSize()
                .padding(.top, 33)
        }
        .frame(width: 82.7, height: 50, alignment: .top)
        .contentShape(Rectangle())
        .onTapGesture {
            if enabled {
                action()
            }
        }
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 30, style: .circular)
        ZStack(alignment: .topLeading) {
            Color.clear
                .contentShape(Rectangle())
                .onTapGesture { model.copyPasteOpen = false }
            ZStack(alignment: .topLeading) {
                Capsule()
                    .fill(Color(argb: 0xFF7C_7C7C))
                    .frame(width: 40, height: 4)
                    .padding(.top, 5)
                    .frame(maxWidth: .infinity)
                Text(verbatim: "Copy & Paste")
                    .font(.system(size: 22, weight: .bold))
                    .foregroundStyle(panelTitleInk())
                    .lineLimit(1)
                    .fixedSize()
                    .offset(x: 18, y: 12)
                PcIconView(PcIcons.cross, size: 18, color: panelButtonInk())
                    .frame(width: 40, height: 40)
                    .background(Circle().fill(panelCloseFill()))
                    .contentShape(Circle())
                    .onTapGesture { model.copyPasteOpen = false }
                    .offset(x: 453, y: 6)
                HStack(spacing: 0) {
                    button(PcIcons.copyCut, "Cut", action: act(model.cutLayer))
                    button(PcIcons.copyOne, "Copy", action: act(model.copyLayer))
                    button(PcIcons.copyAll, "Copy All", action: act(model.copyCanvas))
                    button(PcIcons.copyDuplicate, "Duplicate", action: act(model.duplicateCurrent))
                    button(PcIcons.cutPaste, "Cut & Paste", action: act(model.cutAndPaste))
                    button(PcIcons.pasteDown, "Paste", enabled: model.canPaste, action: act({ model.paste() }))
                }
                .offset(y: 48)
            }
            .frame(width: 496, height: 106, alignment: .topLeading)
            .background(shape.fill(panelFill()))
            .clipShape(shape)
            .contentShape(shape)
            .onTapGesture {}
            .offset(x: (screen.width - 496) / 2, y: screen.height * (617.0 / 1133.0))
        }
    }
}
