// The app's way to WidgetKit. desktop/src-tauri/src/widgets.rs opens this library at run time and
// calls its one function, so the app itself keeps its macOS 11 minimum and links no Swift.
import WidgetKit

/// Redraws one widget by its kind, or every widget when `kind` is null. Each widget's redraws are
/// rationed by the system, so the app names only the ones whose part of the file changed.
@_cdecl("wmpass_reload_widgets")
public func wmpassReloadWidgets(_ kind: UnsafePointer<CChar>?) {
    if let kind {
        WidgetCenter.shared.reloadTimelines(ofKind: String(cString: kind))
    } else {
        WidgetCenter.shared.reloadAllTimelines()
    }
}
