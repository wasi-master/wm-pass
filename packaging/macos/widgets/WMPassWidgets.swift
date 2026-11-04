// The widget extension's entry point. Built by packaging/macos/widgets/build.sh with swiftc alone
// and linked with -e _NSExtensionMain. Each widget's kind is the name WM Pass reloads it by
// (KINDS in desktop/src-tauri/src/widgets.rs).
import SwiftUI
import WidgetKit

struct PassEntry: TimelineEntry {
    let date: Date
    let snapshot: Snapshot?
}

/// One entry now, and one at `hide_after`, when vault content goes even if no newer file comes.
struct PassProvider: TimelineProvider {
    func placeholder(in context: Context) -> PassEntry { PassEntry(date: Date(), snapshot: nil) }

    func getSnapshot(in context: Context, completion: @escaping (PassEntry) -> Void) {
        completion(PassEntry(date: Date(), snapshot: Snapshot.load()))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<PassEntry>) -> Void) {
        let now = Date()
        let snapshot = Snapshot.load()
        var entries = [PassEntry(date: now, snapshot: snapshot)]
        if let hide = snapshot?.hideAfter.map(when), hide > now {
            entries.append(PassEntry(date: hide, snapshot: snapshot))
        }
        completion(Timeline(entries: entries, policy: .after(now.addingTimeInterval(3600))))
    }
}

/// An entry at each code's change, and ten seconds before it for the next code, up to the end of
/// what the file holds. After that the widget says to open WM Pass, never an old code.
struct CodesProvider: TimelineProvider {
    func placeholder(in context: Context) -> PassEntry { PassEntry(date: Date(), snapshot: nil) }

    func getSnapshot(in context: Context, completion: @escaping (PassEntry) -> Void) {
        completion(PassEntry(date: Date(), snapshot: Snapshot.load()))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<PassEntry>) -> Void) {
        let now = Date()
        let snapshot = Snapshot.load()
        var moments: Set<Date> = [now]
        if let s = snapshot, let hide = s.hideAfter.map(when) {
            for code in s.codes {
                for step in code.steps {
                    let end = when(step.until)
                    for moment in [end, end.addingTimeInterval(-10)] where moment > now && moment <= hide {
                        moments.insert(moment)
                    }
                }
            }
            if hide > now { moments.insert(hide) }
        }
        let entries = moments.sorted().prefix(60).map { PassEntry(date: $0, snapshot: snapshot) }
        let end = entries.last?.date ?? now
        completion(Timeline(entries: Array(entries), policy: .after(max(end, now.addingTimeInterval(60)))))
    }
}

private struct Framed<Content: View>: View {
    let content: Content
    init(@ViewBuilder _ content: () -> Content) { self.content = content() }
    var body: some View {
        content.containerBackground(.fill.tertiary, for: .widget)
    }
}

struct QuickWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "quick", provider: PassProvider()) { entry in
            QuickEntryView(entry: entry)
        }
        .configurationDisplayName("Quick bar")
        .description("The buttons you pick in WM Pass: search, add, codes, generator, health and lock. Shows nothing from your vault.")
        .supportedFamilies([.systemSmall, .systemMedium])
    }
}

private struct QuickEntryView: View {
    @Environment(\.widgetFamily) private var family
    let entry: PassEntry
    var body: some View { Framed { QuickView(snapshot: entry.snapshot, date: entry.date, family: family) } }
}

struct StatusWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "status", provider: PassProvider()) { entry in
            Framed { StatusView(snapshot: entry.snapshot, date: entry.date) }
                .widgetURL(StatusView.url(entry.snapshot, entry.date))
        }
        .configurationDisplayName("Lock status")
        .description("Whether the vault is open. Click to lock it, or to open it.")
        .supportedFamilies([.systemSmall])
    }
}

struct HealthWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "health", provider: PassProvider()) { entry in
            HealthEntryView(entry: entry).widgetURL(passLink("health"))
        }
        .configurationDisplayName("Health")
        .description("Your health score and how many passwords need attention. Names no item.")
        .supportedFamilies([.systemSmall, .systemMedium])
    }
}

private struct HealthEntryView: View {
    @Environment(\.widgetFamily) private var family
    let entry: PassEntry
    var body: some View { Framed { HealthView(snapshot: entry.snapshot, family: family) } }
}

struct BackupsWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "backups", provider: PassProvider()) { entry in
            BackupsEntryView(entry: entry).widgetURL(passLink("backups"))
        }
        .configurationDisplayName("Backups")
        .description("When each backup place last ran, and whether it failed.")
        .supportedFamilies([.systemSmall, .systemMedium, .systemLarge])
    }
}

private struct BackupsEntryView: View {
    @Environment(\.widgetFamily) private var family
    let entry: PassEntry
    var body: some View { Framed { BackupsView(snapshot: entry.snapshot, date: entry.date, family: family) } }
}

struct GeneratorWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "generator", provider: PassProvider()) { entry in
            Framed { GeneratorView(snapshot: entry.snapshot) }
                .widgetURL(passLink("generate", token: entry.snapshot?.token))
        }
        .configurationDisplayName("Generator")
        .description("Click to copy a new password. Nothing is shown.")
        .supportedFamilies([.systemSmall])
    }
}

struct CodesWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "codes", provider: CodesProvider()) { entry in
            CodesEntryView(entry: entry).widgetURL(passLink("codes"))
        }
        .configurationDisplayName("Codes")
        .description("One-time codes while the vault is open, if you turn on Show on widgets in WM Pass.")
        .supportedFamilies([.systemSmall, .systemMedium, .systemLarge])
    }
}

private struct CodesEntryView: View {
    @Environment(\.widgetFamily) private var family
    let entry: PassEntry
    var body: some View { Framed { CodesView(snapshot: entry.snapshot, date: entry.date, family: family) } }
}

struct FavoritesWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "favorites", provider: PassProvider()) { entry in
            FavoritesEntryView(entry: entry).widgetURL(passLink("open"))
        }
        .configurationDisplayName("Favorites")
        .description("Your favorites while the vault is open, if you turn on Show on widgets in WM Pass.")
        .supportedFamilies([.systemMedium, .systemLarge])
    }
}

private struct FavoritesEntryView: View {
    @Environment(\.widgetFamily) private var family
    let entry: PassEntry
    var body: some View { Framed { FavoritesView(snapshot: entry.snapshot, date: entry.date, family: family) } }
}

@main
struct WMPassWidgets: WidgetBundle {
    var body: some Widget {
        QuickWidget()
        StatusWidget()
        HealthWidget()
        BackupsWidget()
        GeneratorWidget()
        CodesWidget()
        FavoritesWidget()
    }
}
