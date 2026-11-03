// The widgets' data and views. WM Pass writes snapshot.json into this extension's own folder
// (desktop/src-tauri/src/widgets.rs); the extension holds no key, asks for nothing, and shows the
// last file it was given. Vault content in it is shown only until `hide_after`.
import SwiftUI
import WidgetKit

struct Snapshot: Decodable {
    struct Health: Decodable {
        let score: Int
        let weak: Int
        let reused: Int
        let breached: Int
        let at: Double
        var issues: Int { weak + reused + breached }
    }

    struct Backup: Decodable, Identifiable {
        let label: String
        let last: Double?
        let failed: Bool
        var id: String { label }
    }

    struct Step: Decodable {
        let from: Double
        let until: Double
        let code: String
    }

    struct Code: Decodable, Identifiable {
        let id: String
        let title: String
        let steps: [Step]

        /// The step that covers a moment, if the file reaches that far.
        func step(at date: Date) -> (Step, Step?)? {
            let ms = date.timeIntervalSince1970 * 1000
            guard let index = steps.firstIndex(where: { $0.from <= ms && ms < $0.until }) else { return nil }
            return (steps[index], index + 1 < steps.count ? steps[index + 1] : nil)
        }
    }

    struct Favorite: Decodable, Identifiable {
        let id: String
        let title: String
        let color: String
    }

    let generatedAt: Double
    let token: String
    let state: String
    let vault: String?
    let hideAfter: Double?
    let buttons: [String]
    let lockAction: String
    let access: Bool
    let health: Health?
    let showScore: Bool
    let backups: [Backup]
    let everyDays: Int
    let generator: String
    let codesMode: String
    let codes: [Code]
    let favorites: [Favorite]

    /// Open at this moment, as far as this file knows. Past `hide_after` it is taken as locked.
    func open(at date: Date) -> Bool {
        guard state == "open", let hideAfter else { return false }
        return date.timeIntervalSince1970 * 1000 < hideAfter
    }

    /// NSHomeDirectory() is the extension's container: the sandbox has no other way in.
    static func load() -> Snapshot? {
        let path = (NSHomeDirectory() as NSString).appendingPathComponent("snapshot.json")
        guard let data = FileManager.default.contents(atPath: path) else { return nil }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try? decoder.decode(Snapshot.self, from: data)
    }
}

/// A link back into WM Pass. The app reads only wmpass://widget/... links.
func passLink(_ action: String, token: String? = nil, id: String? = nil) -> URL {
    var parts = URLComponents()
    parts.scheme = "wmpass"
    parts.host = "widget"
    parts.path = "/\(action)"
    var query: [URLQueryItem] = []
    if let token { query.append(URLQueryItem(name: "t", value: token)) }
    if let id { query.append(URLQueryItem(name: "id", value: id)) }
    if !query.isEmpty { parts.queryItems = query }
    return parts.url ?? URL(string: "wmpass://widget/open")!
}

/// The shield's emerald, for the few marks that carry the brand.
let passGreen = Color(red: 0.063, green: 0.725, blue: 0.506)

func spaced(_ code: String) -> String {
    switch code.count {
    case 6: return "\(code.prefix(3)) \(code.suffix(3))"
    case 8: return "\(code.prefix(4)) \(code.suffix(4))"
    default: return code
    }
}

func when(_ ms: Double) -> Date { Date(timeIntervalSince1970: ms / 1000) }

private struct Message: View {
    let symbol: String
    let text: String
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Image(systemName: symbol).font(.title3).foregroundStyle(.secondary)
            Text(text).font(.footnote).foregroundStyle(.secondary)
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
}

private struct Header: View {
    let symbol: String
    let title: String
    var body: some View {
        Label(title, systemImage: symbol)
            .font(.caption)
            .foregroundStyle(.secondary)
            .labelStyle(.titleAndIcon)
    }
}

// ---- quick bar ----

struct QuickButtonSpec {
    let symbol: String
    let label: String
    let url: URL
    let active: Bool
}

func quickButton(_ id: String, _ s: Snapshot, open: Bool) -> QuickButtonSpec {
    switch id {
    case "search": return QuickButtonSpec(symbol: "magnifyingglass", label: "Search", url: passLink("search"), active: false)
    case "add": return QuickButtonSpec(symbol: "plus", label: "Add", url: passLink("add"), active: false)
    case "codes": return QuickButtonSpec(symbol: "number.square", label: "Codes", url: passLink("codes"), active: false)
    case "generator": return QuickButtonSpec(symbol: "dice", label: "Generator", url: passLink("generator"), active: false)
    case "health": return QuickButtonSpec(symbol: "heart.text.square", label: "Health", url: passLink("health"), active: false)
    default:
        return open
            ? QuickButtonSpec(symbol: "lock.open.fill", label: "Lock", url: passLink("lock", token: s.token), active: true)
            : QuickButtonSpec(symbol: "lock.fill", label: "Locked", url: passLink("open"), active: false)
    }
}

struct QuickView: View {
    let snapshot: Snapshot?
    let date: Date
    let family: WidgetFamily

    var body: some View {
        if let s = snapshot {
            let open = s.open(at: date)
            let ids = s.buttons.isEmpty ? ["search", "add", "codes", "lock"] : s.buttons
            let buttons = ids.prefix(family == .systemSmall ? 4 : 6).map { quickButton($0, s, open: open) }
            if family == .systemSmall {
                LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 10) {
                    ForEach(Array(buttons.enumerated()), id: \.offset) { _, b in QuickButton(spec: b, labelled: false) }
                }
            } else {
                HStack(spacing: 6) {
                    ForEach(Array(buttons.enumerated()), id: \.offset) { _, b in QuickButton(spec: b, labelled: true) }
                }
            }
        } else {
            Message(symbol: "lock.shield", text: "Open WM Pass once to set up its widgets.")
        }
    }
}

private struct QuickButton: View {
    let spec: QuickButtonSpec
    let labelled: Bool
    var body: some View {
        Link(destination: spec.url) {
            VStack(spacing: 4) {
                Image(systemName: spec.symbol)
                    .font(.title3)
                    .frame(width: 44, height: 44)
                    .background(Circle().fill(spec.active ? AnyShapeStyle(passGreen.opacity(0.25)) : AnyShapeStyle(.quaternary)))
                    .foregroundStyle(spec.active ? AnyShapeStyle(passGreen) : AnyShapeStyle(.primary))
                    .widgetAccentable(spec.active)
                if labelled {
                    Text(spec.label).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            .frame(maxWidth: .infinity)
        }
        .accessibilityLabel(spec.label)
    }
}

// ---- lock status ----

struct StatusView: View {
    let snapshot: Snapshot?
    let date: Date

    var body: some View {
        let open = snapshot?.open(at: date) ?? false
        let panic = snapshot?.lockAction == "panic"
        VStack(alignment: .leading, spacing: 4) {
            Image(systemName: open ? "lock.open.fill" : "lock.fill")
                .font(.title)
                .foregroundStyle(open ? AnyShapeStyle(passGreen) : AnyShapeStyle(.secondary))
                .widgetAccentable()
            Spacer(minLength: 0)
            Text(open ? "Open" : (snapshot?.state == "none" ? "No vault" : "Locked")).font(.headline)
            if open, let vault = snapshot?.vault {
                Text(vault).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Text(open ? (panic ? "Click for a panic lock" : "Click to lock") : "Click to open")
                .font(.caption2).foregroundStyle(.secondary).lineLimit(1)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    static func url(_ snapshot: Snapshot?, _ date: Date) -> URL {
        guard let s = snapshot, s.open(at: date) else { return passLink("open") }
        return passLink("lock", token: s.token)
    }
}

// ---- health ----

struct HealthView: View {
    let snapshot: Snapshot?
    let family: WidgetFamily

    var body: some View {
        if let h = snapshot?.health {
            let verdict = h.issues == 0 ? "All good" : (h.issues == 1 ? "1 to fix" : "\(h.issues) to fix")
            VStack(alignment: .leading, spacing: 4) {
                Header(symbol: "heart.text.square", title: "Health")
                Spacer(minLength: 0)
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    if snapshot?.showScore ?? true {
                        Text(verbatim: "\(h.score)").font(.system(size: 34, weight: .semibold)).monospacedDigit()
                    }
                    Text(verdict).font(.subheadline.weight(.medium))
                        .foregroundStyle(h.issues == 0 ? AnyShapeStyle(passGreen) : AnyShapeStyle(.orange))
                }
                if family != .systemSmall {
                    Text(verbatim: "\(h.weak) weak · \(h.reused) reused · \(h.breached) breached")
                        .font(.caption).foregroundStyle(.secondary)
                }
                (Text("Checked ") + Text(when(h.at), style: .relative) + Text(" ago"))
                    .font(.caption2).foregroundStyle(.secondary).lineLimit(1)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        } else {
            Message(symbol: "heart.text.square", text: "Open WM Pass to check your passwords.")
        }
    }
}

// ---- backups ----

struct BackupsView: View {
    let snapshot: Snapshot?
    let date: Date
    let family: WidgetFamily

    private var room: Int {
        switch family {
        case .systemSmall: return 2
        case .systemMedium: return 3
        default: return 8
        }
    }

    var body: some View {
        if let s = snapshot, !s.backups.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                Header(symbol: "externaldrive.badge.icloud", title: "Backups")
                ForEach(s.backups.prefix(room)) { row in
                    BackupRowView(row: row, everyDays: s.everyDays, date: date, wide: family != .systemSmall)
                }
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        } else {
            Message(symbol: "externaldrive.badge.icloud", text: "No backup places yet. Click to add one.")
        }
    }
}

private struct BackupRowView: View {
    let row: Snapshot.Backup
    let everyDays: Int
    let date: Date
    let wide: Bool

    var body: some View {
        let late = everyDays > 0 && (row.last.map { date.timeIntervalSince(when($0)) > Double(everyDays + 1) * 86_400 } ?? true)
        let (symbol, tint): (String, Color) = row.failed
            ? ("xmark.octagon.fill", .red)
            : (row.last == nil ? ("clock", .orange) : (late ? ("exclamationmark.triangle.fill", .orange) : ("checkmark.circle.fill", passGreen)))
        HStack(spacing: 6) {
            Image(systemName: symbol).foregroundStyle(tint).font(.footnote)
            if wide {
                Text(row.label).font(.footnote).lineLimit(1)
                Spacer(minLength: 4)
                detail.font(.caption).foregroundStyle(.secondary).lineLimit(1)
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    Text(row.label).font(.footnote).lineLimit(1)
                    detail.font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                }
            }
        }
    }

    private var detail: Text {
        guard let last = row.last else { return Text("Never") }
        let prefix = row.failed ? "Failed " : ""
        return Text(prefix) + Text(when(last), format: .relative(presentation: .named))
    }
}

// ---- generator ----

struct GeneratorView: View {
    let snapshot: Snapshot?

    var body: some View {
        let what = snapshot?.generator ?? "password"
        VStack(alignment: .leading, spacing: 4) {
            Image(systemName: "dice").font(.title).foregroundStyle(passGreen).widgetAccentable()
            Spacer(minLength: 0)
            Text("New \(what)").font(.headline)
            Text("Click to copy one").font(.caption2).foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
}

// ---- codes ----

/// Why vault content is not shown, or nil if it may be.
func blocked(_ snapshot: Snapshot?, _ date: Date, _ what: String) -> (String, String)? {
    guard let s = snapshot else { return ("lock.shield", "Open WM Pass once to set up its widgets.") }
    if !s.access { return ("eye.slash", "Turn on Show on widgets in WM Pass, under Security, to see \(what) here.") }
    if !s.open(at: date) { return ("lock.fill", "Locked. Open WM Pass to see \(what).") }
    return nil
}

struct CodesView: View {
    let snapshot: Snapshot?
    let date: Date
    let family: WidgetFamily

    private var room: Int {
        switch family {
        case .systemSmall: return 1
        case .systemMedium: return 2
        default: return 6
        }
    }

    var body: some View {
        if let stop = blocked(snapshot, date, "codes") {
            Message(symbol: stop.0, text: stop.1)
        } else if let s = snapshot, s.codes.isEmpty {
            Message(symbol: "number.square", text: "Star an item that has a code, or pick codes in WM Pass under Settings, Widgets.")
        } else if let s = snapshot {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(s.codes.prefix(room)) { code in
                    CodeRowView(code: code, date: date)
                }
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
    }
}

private struct CodeRowView: View {
    let code: Snapshot.Code
    let date: Date

    var body: some View {
        if let (step, next) = code.step(at: date) {
            let ends = when(step.until)
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 0) {
                    Text(code.title).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    Text(spaced(step.code)).font(.system(.title2, design: .monospaced).weight(.medium)).lineLimit(1)
                }
                Spacer(minLength: 4)
                VStack(alignment: .trailing, spacing: 0) {
                    Text(timerInterval: date...ends, countsDown: true)
                        .font(.caption).monospacedDigit().foregroundStyle(.secondary).frame(maxWidth: 44, alignment: .trailing)
                    if let next, ends.timeIntervalSince(date) <= 10 {
                        Text("Next \(spaced(next.code))").font(.caption2.monospacedDigit()).foregroundStyle(.secondary)
                    }
                }
            }
        } else {
            VStack(alignment: .leading, spacing: 0) {
                Text(code.title).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                Text("Open WM Pass to see this code").font(.footnote).foregroundStyle(.secondary)
            }
        }
    }
}

// ---- favorites ----

struct FavoritesView: View {
    let snapshot: Snapshot?
    let date: Date
    let family: WidgetFamily

    var body: some View {
        if let stop = blocked(snapshot, date, "favorites") {
            Message(symbol: stop.0, text: stop.1)
        } else if let s = snapshot, s.favorites.isEmpty {
            Message(symbol: "star", text: "No favorites yet. Star an item to put it here.")
        } else if let s = snapshot {
            let count = family == .systemLarge ? 12 : 4
            LazyVGrid(columns: Array(repeating: GridItem(.flexible()), count: 4), spacing: 10) {
                ForEach(s.favorites.prefix(count)) { item in
                    Link(destination: passLink("item", id: item.id)) {
                        VStack(spacing: 4) {
                            Text(String(item.title.first(where: { $0.isLetter || $0.isNumber }).map { String($0).uppercased() } ?? "•"))
                                .font(.title3.weight(.semibold))
                                .foregroundStyle(.white)
                                .frame(width: 40, height: 40)
                                .background(Circle().fill(Color(hex: item.color) ?? passGreen))
                            Text(item.title).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                    .accessibilityLabel("Open \(item.title)")
                }
            }
            .frame(maxHeight: .infinity, alignment: .center)
        }
    }
}

extension Color {
    /// `#rrggbb`, or nil.
    init?(hex: String) {
        let digits = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
        guard digits.count == 6, let value = UInt32(digits, radix: 16) else { return nil }
        self.init(red: Double((value >> 16) & 0xFF) / 255, green: Double((value >> 8) & 0xFF) / 255, blue: Double(value & 0xFF) / 255)
    }
}
